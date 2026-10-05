//! Admin panel: hozzáférés-szabályozás (a Python `test_admin_access.py` megfelelője).
//!
//! Alapelv: aki nem admin, az semmilyen módon nem tudhatja meg, hogy admin panel létezik — minden admin útvonal
//! (oldal, asset, API, rossz metódus, nem létező alútvonal) ugyanazt a 404-et adja, mint egy nem létező cím.

mod common;
use common::admin::*;
use common::*;
use scrabble::admin;
use scrabble::config::parse_ip_networks;
use serde_json::{Value, json};
use std::path::Path;

const ROOT: &str = env!("CARGO_MANIFEST_DIR");

/// Minden admin végpont, metódussal (a rossz metódusúak is: a 405 sem árulkodhat)
const ADMIN_ENDPOINTS: &[(&str, &str)] = &[
    ("GET", "/admin"),
    ("GET", "/admin/assets/admin.js"),
    ("GET", "/admin/assets/admin.css"),
    ("GET", "/admin/assets/admin-i18n.js"),
    ("GET", "/admin/assets/admin-boot.js"),
    ("GET", "/admin/assets/admin-ui.js"),
    ("GET", "/admin/assets/admin-views-a.js"),
    ("GET", "/admin/assets/admin-views-b.js"),
    ("GET", "/admin/assets/admin-views-c.js"),
    ("GET", "/admin/assets/admin-main.js"),
    ("GET", "/admin/assets/nincs-ilyen.js"),
    ("GET", "/admin/masik"),
    ("GET", "/api/admin"),
    ("GET", "/api/admin/session"),
    ("POST", "/api/admin/session"),
    ("GET", "/api/admin/audit"),
    ("GET", "/api/admin/audit?format=csv"),
    ("GET", "/api/admin/audit?download=1"),
    ("DELETE", "/api/admin/audit"),
    ("POST", "/api/admin/reauth"),
    ("GET", "/api/admin/reauth"),
    ("POST", "/api/admin/sudo"),
    ("DELETE", "/api/admin/sudo"),
    ("GET", "/api/admin/sudo"),
    ("GET", "/api/admin/nincs-ilyen"),
    ("POST", "/api/admin/users/1/ban"),
];

const UNSAFE: [&str; 4] = ["POST", "PUT", "PATCH", "DELETE"];

fn csrf_headers(server: &TestServer) -> Vec<(&'static str, String)> {
    vec![("X-Admin-Request", "1".to_string()), ("Origin", server.base())]
}

async fn call(http: &Http, server: &TestServer, method: &str, path: &str) -> Resp {
    let headers = csrf_headers(server);
    let refs: Vec<(&str, &str)> = headers.iter().map(|(k, v)| (*k, v.as_str())).collect();
    http.request(method, path, if UNSAFE.contains(&method) { Some(json!({})) } else { None }, &refs).await
}

async fn reference_404(server: &TestServer) -> Resp {
    let response = server.http().get("/ez-az-oldal-nem-letezik").await;
    assert_eq!(response.status, 404);
    response
}

fn assert_plain_404(response: &Resp, reference: &Resp, what: &str) {
    assert_eq!(response.status, 404, "{what}");
    assert_eq!(response.body, reference.body, "{what}");
    for header in ["Content-Type", "Content-Length"] {
        assert_eq!(response.header(header), reference.header(header), "{what}: {header}");
    }
    // Az admin válaszok fejlécei sem szivároghatnak a 404-re
    for header in ["Cache-Control", "X-Frame-Options", "Content-Security-Policy", "X-Robots-Tag"] {
        assert!(response.header(header).is_none(), "{what}: {header}");
    }
}

/// Minden admin útvonal és minden megengedett metódusa az alkalmazás útvonaltérképéből (mintaazonosítókkal), plusz egy
/// nem engedélyezett metódus (PUT): új végpont így automatikusan bekerül a hozzáférési tesztekbe.
fn all_admin_routes(server: &TestServer) -> Vec<(String, String)> {
    let param = regex::Regex::new(r"\{[^}]+\}").unwrap();
    let mut items: std::collections::BTreeSet<(String, String)> = std::collections::BTreeSet::new();
    let mut paths: Vec<(String, String)> = scrabble::admin::routes::route_table(&server.app)
        .into_iter()
        .map(|(method, path)| {
            let path = if path.starts_with("/api/admin") || path.starts_with("/admin") { path } else { format!("/api/admin{path}") };
            (method.to_string(), param.replace_all(&path, "1").to_string())
        })
        .collect();
    paths.push(("GET".into(), "/admin/assets/1".into()));
    for (method, path) in paths {
        items.insert(("PUT".to_string(), path.clone()));
        items.insert((method, path));
    }
    items.into_iter().collect()
}

// ===================================================================================================
// A teljes útvonaltérkép
// ===================================================================================================

#[tokio::test(flavor = "multi_thread")]
async fn the_route_map_is_complete() {
    let server = TestServer::start().await;
    let routes = all_admin_routes(&server);
    assert!(routes.len() > 200, "{}", routes.len());
    assert!(routes.contains(&("GET".into(), "/api/admin/overview".into())));
    assert!(routes.contains(&("POST".into(), "/api/admin/rooms/1/action".into())));
}

#[tokio::test(flavor = "multi_thread")]
async fn anonymous_gets_the_plain_404_everywhere() {
    let server = TestServer::start().await;
    let reference = reference_404(&server).await;
    let http = server.http();
    for (method, path) in all_admin_routes(&server) {
        let response = call(&http, &server, &method, &path).await;
        assert_plain_404(&response, &reference, &format!("{method} {path}"));
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_normal_user_gets_the_plain_404_everywhere() {
    let server = TestServer::start().await;
    let reference = reference_404(&server).await;
    let (_, http) = make_user(&server, "player@example.com", "Játékos").await;
    for (method, path) in all_admin_routes(&server) {
        let response = call(&http, &server, &method, &path).await;
        assert_plain_404(&response, &reference, &format!("{method} {path}"));
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn every_unsafe_request_needs_the_csrf_header() {
    let server = TestServer::start().await;
    let http = server.admin_http().await;
    for (method, path) in all_admin_routes(&server) {
        if UNSAFE.contains(&method.as_str()) {
            let response = http.request(&method, &path, Some(json!({})), &[("Origin", &server.base())]).await;
            assert_eq!(response.status, 403, "{method} {path}");
        }
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn every_unsafe_request_needs_a_same_origin_header() {
    let server = TestServer::start().await;
    let http = server.admin_http().await;
    for (method, path) in all_admin_routes(&server) {
        if UNSAFE.contains(&method.as_str()) {
            let response = http.request(&method, &path, Some(json!({})), &[("X-Admin-Request", "1"), ("Origin", "http://evil.example")]).await;
            assert_eq!(response.status, 403, "{method} {path}");
        }
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn an_idle_session_needs_the_password_on_every_api_route() {
    let server = TestServer::start().await;
    let http = server.admin_http().await;
    let token = http.session_token().unwrap();
    set_session(&server, &token, "admin_seen_at", Some("2000-01-01 00:00:00"));
    for (method, path) in all_admin_routes(&server) {
        if !path.starts_with("/api/admin") || path == "/api/admin/session" || path == "/api/admin/reauth" || method == "PUT" {
            continue;
        }
        let response = call(&http, &server, &method, &path).await;
        assert_eq!(response.status, 401, "{method} {path}");
        assert_eq!(response.json()["reauth"], true, "{method} {path}");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn admin_responses_carry_the_security_headers() {
    let server = TestServer::start().await;
    let http = server.admin_http().await;
    for path in ["/admin", "/api/admin/overview", "/api/admin/system", "/admin/assets/admin-main.js"] {
        let response = http.get(path).await;
        assert_eq!(response.status, 200, "{path}");
        assert_eq!(response.header("Cache-Control").as_deref(), Some("no-store"), "{path}");
        assert_eq!(response.header("X-Frame-Options").as_deref(), Some("DENY"), "{path}");
        assert!(response.header("Content-Security-Policy").unwrap().contains("frame-ancestors 'none'"), "{path}");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn the_csp_allows_only_the_socket_io_cdn_besides_self() {
    let server = TestServer::start().await;
    let http = server.admin_http().await;
    let csp = http.get("/admin").await.header("Content-Security-Policy").unwrap();
    assert!(csp.contains("script-src 'self' https://cdnjs.cloudflare.com;"), "{csp}");
    assert!(csp.contains("style-src 'self';") && !csp.contains("unsafe-inline") && !csp.contains("unsafe-eval"), "{csp}");
    assert!(csp.contains("default-src 'none'"), "{csp}");
}

// ===================================================================================================
// Aki nem admin, semmit sem lát
// ===================================================================================================

#[tokio::test(flavor = "multi_thread")]
async fn anonymous_sees_nothing() {
    let server = TestServer::start().await;
    let reference = reference_404(&server).await;
    for (method, path) in ADMIN_ENDPOINTS {
        let response = call(&server.http(), &server, method, path).await;
        assert_plain_404(&response, &reference, &format!("{method} {path}"));
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn an_invalid_session_token_sees_nothing() {
    let server = TestServer::start().await;
    let reference = reference_404(&server).await;
    let http = server.http();
    http.set_cookie(Some("ervenytelen-token"));
    for (method, path) in ADMIN_ENDPOINTS {
        let response = call(&http, &server, method, path).await;
        assert_plain_404(&response, &reference, &format!("{method} {path}"));
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_registered_non_admin_sees_nothing() {
    let server = TestServer::start().await;
    let reference = reference_404(&server).await;
    let (_, http) = make_user(&server, "player@example.com", "Játékos").await;
    for (method, path) in ADMIN_ENDPOINTS {
        let response = call(&http, &server, method, path).await;
        assert_plain_404(&response, &reference, &format!("{method} {path}"));
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn an_admin_account_sees_nothing_while_the_admin_list_is_empty() {
    let server = TestServer::start_with(|c| c.admin_emails.clear()).await;
    let reference = reference_404(&server).await;
    let http = server.admin_http().await;
    for (method, path) in ADMIN_ENDPOINTS {
        let response = call(&http, &server, method, path).await;
        assert_plain_404(&response, &reference, &format!("{method} {path}"));
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn double_slashes_do_not_bypass_the_guard() {
    let server = TestServer::start().await;
    let http = server.http();
    let reference = http.get("//ez-az-oldal-nem-letezik").await;
    for path in ["//admin", "/admin//assets/admin.js", "//api/admin/session", "/api//admin/session", "/api/admin//audit"] {
        let response = http.get(path).await;
        assert_eq!((response.status, response.header("Location"), &response.body), (reference.status, reference.header("Location"), &reference.body), "{path}");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn the_display_name_does_not_make_an_admin() {
    // Az adminság az e-mail címhez kötött: a megjelenítési név átírható, ezért nem számít
    let server = TestServer::start().await;
    let reference = reference_404(&server).await;
    let (_, http) = make_user(&server, "masik@example.com", ADMIN_EMAIL).await;
    assert_plain_404(&http.get("/api/admin/session").await, &reference, "megjelenítési név");
}

#[tokio::test(flavor = "multi_thread")]
async fn an_expired_session_is_not_admin() {
    let server = TestServer::start().await;
    let reference = reference_404(&server).await;
    let http = server.admin_http().await;
    set_session(&server, &http.session_token().unwrap(), "expires_at", Some("2000-01-01 00:00:00"));
    assert_plain_404(&http.get("/api/admin/session").await, &reference, "lejárt munkamenet");
}

// ===================================================================================================
// Az admin hozzáférése
// ===================================================================================================

#[tokio::test(flavor = "multi_thread")]
async fn the_session_endpoint() {
    let server = TestServer::start().await;
    let http = server.admin_http().await;
    let response = http.get("/api/admin/session").await;
    assert_eq!(response.status, 200);
    let data = response.json();
    assert_eq!(data["admin"]["display_name"], "Admin");
    assert_eq!(data["admin"]["email"], ADMIN_EMAIL);
    assert_eq!(data["session"]["idle_expired"], false);
    assert_eq!(data["session"]["sudo_remaining"], 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn the_email_comparison_is_case_insensitive() {
    let server = TestServer::start().await;
    server.create_user("Admin@Example.COM", "Nagybetű");
    let http = server.http_as("Admin@Example.COM").await;
    assert_eq!(http.get("/api/admin/session").await.status, 200);
}

#[tokio::test(flavor = "multi_thread")]
async fn the_admin_page_and_its_headers() {
    let server = TestServer::start().await;
    let http = server.admin_http().await;
    let response = http.get("/admin").await;
    assert_eq!(response.status, 200);
    let body = response.text();
    assert!(body.contains("id=\"admin-app\""));
    assert!(body.contains("<meta name=\"robots\" content=\"noindex,nofollow\">"));
    assert_eq!(response.header("Cache-Control").as_deref(), Some("no-store"));
    assert_eq!(response.header("X-Frame-Options").as_deref(), Some("DENY"));
    assert!(response.header("X-Robots-Tag").unwrap().contains("noindex"));
    let csp = response.header("Content-Security-Policy").unwrap();
    assert!(csp.contains("script-src 'self'") && csp.contains("frame-ancestors 'none'") && !csp.contains("unsafe-inline"), "{csp}");
}

#[tokio::test(flavor = "multi_thread")]
async fn the_page_has_no_inline_script_or_style() {
    let server = TestServer::start().await;
    let http = server.admin_http().await;
    let body = http.get("/admin").await.text();
    let script = regex::Regex::new(r"<script([^>]*)>").unwrap();
    for caps in script.captures_iter(&body) {
        assert!(caps[1].contains("src="), "beágyazott szkript: {}", &caps[0]);
    }
    assert!(!regex::Regex::new(r"\sstyle=").unwrap().is_match(&body));
    assert!(!regex::Regex::new(r"\son[a-z]+=").unwrap().is_match(&body));
}

#[tokio::test(flavor = "multi_thread")]
async fn the_assets_are_served_to_the_admin() {
    let server = TestServer::start().await;
    let http = server.admin_http().await;
    for (name, mime) in [
        ("admin.js", "javascript"),
        ("admin.css", "css"),
        ("admin-i18n.js", "javascript"),
        ("admin-boot.js", "javascript"),
        ("admin-ui.js", "javascript"),
        ("admin-views-a.js", "javascript"),
        ("admin-views-b.js", "javascript"),
        ("admin-views-c.js", "javascript"),
        ("admin-main.js", "javascript"),
    ] {
        let response = http.get(&format!("/admin/assets/{name}")).await;
        assert_eq!(response.status, 200, "{name}");
        assert!(response.header("Content-Type").unwrap().contains(mime), "{name}");
        assert_eq!(response.header("Cache-Control").as_deref(), Some("no-store"), "{name}");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn asset_path_traversal_is_rejected() {
    let server = TestServer::start().await;
    let http = server.admin_http().await;
    for path in ["/admin/assets/../auth.py", "/admin/assets/%2e%2e/Cargo.toml", "/admin/assets/..%2fCargo.toml", "/admin/assets/../src/main.rs"] {
        assert_eq!(http.get(path).await.status, 404, "{path}");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn an_unknown_admin_route_is_a_plain_404_for_the_admin_too() {
    let server = TestServer::start().await;
    let http = server.admin_http().await;
    assert_eq!(http.get("/api/admin/nincs-ilyen").await.status, 404);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_wrong_method_is_405_only_for_the_admin() {
    let server = TestServer::start().await;
    let http = server.admin_http().await;
    assert_eq!(http.admin_post("/session", json!({})).await.status, 405);
}

// ===================================================================================================
// Az admin jelző csak az adminnak
// ===================================================================================================

#[tokio::test(flavor = "multi_thread")]
async fn me_has_no_admin_key_for_a_normal_user() {
    let server = TestServer::start().await;
    let (_, http) = make_user(&server, "player@example.com", "Játékos").await;
    let user = http.get("/api/auth/me").await.json()["user"].clone();
    assert!(user.get("is_admin").is_none(), "{user}");
}

#[tokio::test(flavor = "multi_thread")]
async fn me_has_the_flag_for_the_admin() {
    let server = TestServer::start().await;
    let http = server.admin_http().await;
    assert_eq!(http.get("/api/auth/me").await.json()["user"]["is_admin"], true);
}

#[tokio::test(flavor = "multi_thread")]
async fn the_login_response_has_the_flag_only_for_the_admin() {
    let server = TestServer::start().await;
    server.create_user("player@example.com", "Játékos");
    server.create_user(ADMIN_EMAIL, "Főnök");
    let normal = server.http().post("/api/auth/login", json!({"email": "player@example.com", "password": PASSWORD})).await.json();
    assert!(normal["user"].get("is_admin").is_none(), "{normal}");
    let boss = server.http().post("/api/auth/login", json!({"email": ADMIN_EMAIL, "password": PASSWORD})).await.json();
    assert_eq!(boss["user"]["is_admin"], true);
}

#[tokio::test(flavor = "multi_thread")]
async fn registering_the_admin_email() {
    let server = TestServer::start().await;
    let code = server.app.db.create_verification_code(ADMIN_EMAIL).unwrap();
    assert!(server.app.db.verify_code(ADMIN_EMAIL, &code).0);
    let response = server.http().post("/api/auth/register", json!({"email": ADMIN_EMAIL, "password": PASSWORD, "display_name": "Új Admin"})).await;
    assert_eq!(response.status, 200, "{}", response.text());
    assert_eq!(response.json()["user"]["is_admin"], true);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_fresh_login_opens_the_panel_without_asking_the_password_again() {
    let server = TestServer::start().await;
    server.create_user(ADMIN_EMAIL, "Főnök");
    let http = server.http();
    assert_eq!(http.post("/api/auth/login", json!({"email": ADMIN_EMAIL, "password": PASSWORD})).await.status, 200);
    assert_eq!(http.get("/api/admin/audit").await.status, 200);
}

#[test]
fn nobody_without_an_email_is_an_admin() {
    let admins = std::collections::HashSet::from([ADMIN_EMAIL.to_string()]);
    assert!(scrabble::db::is_admin_email(ADMIN_EMAIL, &admins));
    assert!(scrabble::db::is_admin_email("ADMIN@example.COM", &admins));
    assert!(!scrabble::db::is_admin_email("", &admins));
    assert!(!scrabble::db::is_admin_email("masik@example.com", &admins));
    assert!(!scrabble::db::is_admin_email(ADMIN_EMAIL, &std::collections::HashSet::new()));
}

// ===================================================================================================
// Az admin e-mail cím nem foglalható le
// ===================================================================================================

#[tokio::test(flavor = "multi_thread")]
async fn the_dev_code_is_withheld_for_the_admin_address() {
    // SMTP nélkül a `request-code` a kódot a válaszban adja vissza (fejlesztői mód): az admin címnél ez nem lehet így,
    // különben bárki megerősíthetné az (még nem regisztrált) admin címet és admin lenne
    let server = TestServer::start().await;
    let response = server.http().post("/api/auth/request-code", json!({"email": ADMIN_EMAIL})).await;
    assert_eq!(response.status, 200);
    assert_eq!(response.json()["success"], true);
    assert!(response.json().get("dev_code").is_none());
}

#[tokio::test(flavor = "multi_thread")]
async fn case_variants_of_the_admin_address_too() {
    let server = TestServer::start().await;
    let response = server.http().post("/api/auth/request-code", json!({"email": "ADMIN@Example.com"})).await;
    assert!(response.json().get("dev_code").is_none());
}

#[tokio::test(flavor = "multi_thread")]
async fn other_addresses_keep_the_development_convenience() {
    let server = TestServer::start().await;
    let response = server.http().post("/api/auth/request-code", json!({"email": "player@example.com"})).await;
    let code = response.json()["dev_code"].as_str().unwrap().to_string();
    assert!(code.len() == 6 && code.chars().all(|c| c.is_ascii_digit()), "{code}");
}

#[tokio::test(flavor = "multi_thread")]
async fn the_code_is_still_valid_for_whoever_reads_the_server_console() {
    let server = TestServer::start().await;
    server.http().post("/api/auth/request-code", json!({"email": ADMIN_EMAIL})).await;
    let code: String = server.app.db.with(|tx| tx.query_row("SELECT code FROM verification_codes WHERE email = ?", [ADMIN_EMAIL], |r| r.get(0))).unwrap();
    let response = server.http().post("/api/auth/verify-code", json!({"email": ADMIN_EMAIL, "code": code})).await;
    assert_eq!(response.status, 200);
}

// ===================================================================================================
// A nyilvános felületen nincs admin
// ===================================================================================================

fn read(relative: &str) -> String {
    std::fs::read_to_string(Path::new(ROOT).join(relative)).unwrap_or_else(|e| panic!("{relative}: {e}"))
}

#[test]
fn index_html_has_no_admin_element() {
    assert!(!read("web/templates/index.html").to_lowercase().contains("admin"));
}

#[tokio::test(flavor = "multi_thread")]
async fn the_served_index_has_no_admin_element() {
    let server = TestServer::start().await;
    let body = server.http().get("/").await.text();
    assert!(!body.to_lowercase().contains("admin"));
}

#[test]
fn the_public_i18n_has_no_admin_text() {
    let source = read("web/static/i18n-data.js");
    assert!(!source.contains("\"admin."));
    assert!(!source.to_lowercase().contains("admin"));
}

#[test]
fn app_js_only_has_the_entry_point() {
    let source = read("web/static/app.js");
    assert!(!source.contains("web/admin") && !source.contains("admin-i18n"));
    assert!(!source.contains("t('admin"));
    assert!(!source.contains("/api/admin"), "az API-t csak a panel hívja");
}

#[test]
fn no_admin_file_in_the_public_static_folder() {
    fn walk(dir: &Path, found: &mut Vec<String>) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            if path.is_dir() {
                walk(&path, found);
            } else if entry.file_name().to_string_lossy().to_lowercase().contains("admin") {
                found.push(path.display().to_string());
            }
        }
    }
    let mut found = Vec::new();
    walk(&Path::new(ROOT).join("web/static"), &mut found);
    assert!(found.is_empty(), "{found:?}");
}

#[tokio::test(flavor = "multi_thread")]
async fn the_public_static_route_does_not_serve_admin_assets() {
    let server = TestServer::start().await;
    let http = server.admin_http().await;
    for name in ["admin.js", "admin.css", "admin-i18n.js", "admin-ui.js", "admin-views-a.js", "admin-views-b.js", "admin-views-c.js", "admin-main.js"] {
        assert_eq!(http.get(&format!("/static/{name}")).await.status, 404, "{name}");
    }
}

// ===================================================================================================
// Tétlenség és újraigazolás
// ===================================================================================================

async fn admin_with_token(server: &TestServer) -> (Http, String) {
    let http = server.admin_http().await;
    let token = http.session_token().unwrap();
    (http, token)
}

#[tokio::test(flavor = "multi_thread")]
async fn an_idle_session_needs_the_password() {
    let server = TestServer::start().await;
    let (http, token) = admin_with_token(&server).await;
    set_session(&server, &token, "admin_seen_at", Some(&ts_ago(30 * 60 + 5)));
    let response = http.get("/api/admin/audit").await;
    assert_eq!(response.status, 401);
    assert_eq!(response.json()["reauth"], true);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_never_used_admin_session_needs_the_password() {
    let server = TestServer::start().await;
    let (http, token) = admin_with_token(&server).await;
    set_session(&server, &token, "admin_seen_at", None);
    assert_eq!(http.get("/api/admin/audit").await.status, 401);
}

#[tokio::test(flavor = "multi_thread")]
async fn the_page_and_status_work_when_idle_and_do_not_refresh_the_timer() {
    let server = TestServer::start().await;
    let (http, token) = admin_with_token(&server).await;
    let stamp = ts_ago(30 * 60 + 5);
    set_session(&server, &token, "admin_seen_at", Some(&stamp));
    assert_eq!(http.get("/admin").await.status, 200);
    assert_eq!(http.get("/admin/assets/admin.js").await.status, 200);
    let status = http.get("/api/admin/session").await;
    assert_eq!(status.status, 200);
    assert_eq!(status.json()["session"]["idle_expired"], true);
    assert_eq!(admin_seen(&server, &token), Some(stamp));
}

#[tokio::test(flavor = "multi_thread")]
async fn the_idle_limit_is_configurable() {
    let server = TestServer::start_with(|c| c.admin_session_idle_minutes = 1).await;
    let (http, token) = admin_with_token(&server).await;
    set_session(&server, &token, "admin_seen_at", Some(&ts_ago(30)));
    assert_eq!(http.get("/api/admin/audit").await.status, 200);
    set_session(&server, &token, "admin_seen_at", Some(&ts_ago(90)));
    assert_eq!(http.get("/api/admin/audit").await.status, 401);
}

#[tokio::test(flavor = "multi_thread")]
async fn activity_refreshes_the_timer() {
    let server = TestServer::start().await;
    let (http, token) = admin_with_token(&server).await;
    let stamp = ts_ago(120);
    set_session(&server, &token, "admin_seen_at", Some(&stamp));
    assert_eq!(http.get("/api/admin/audit").await.status, 200);
    assert!(admin_seen(&server, &token).unwrap() > stamp);
}

#[tokio::test(flavor = "multi_thread")]
async fn timer_writes_are_throttled() {
    let server = TestServer::start().await;
    let (http, token) = admin_with_token(&server).await;
    let stamp = ts_ago(2);
    set_session(&server, &token, "admin_seen_at", Some(&stamp));
    assert_eq!(http.get("/api/admin/audit").await.status, 200);
    assert_eq!(admin_seen(&server, &token), Some(stamp));
}

#[tokio::test(flavor = "multi_thread")]
async fn reauth_with_the_right_password() {
    let server = TestServer::start().await;
    let (http, token) = admin_with_token(&server).await;
    set_session(&server, &token, "admin_seen_at", Some(&ts_ago(30 * 60 + 5)));
    let response = http.admin_post("/reauth", json!({"password": PASSWORD})).await;
    assert_eq!(response.status, 200);
    assert_eq!(response.json()["session"]["idle_expired"], false);
    assert_eq!(http.get("/api/admin/audit").await.status, 200);
    assert_eq!(audit_actions(&server), vec!["admin.reauth"]);
}

#[tokio::test(flavor = "multi_thread")]
async fn reauth_with_a_wrong_password_is_logged() {
    let server = TestServer::start().await;
    let (http, token) = admin_with_token(&server).await;
    set_session(&server, &token, "admin_seen_at", Some(&ts_ago(30 * 60 + 5)));
    let response = http.admin_post("/reauth", json!({"password": "rossz-jelszo"})).await;
    assert_eq!(response.status, 403);
    assert_eq!(http.get("/api/admin/audit").await.status, 401);
    assert_eq!(audit_actions(&server), vec!["admin.reauth_failed"]);
}

#[tokio::test(flavor = "multi_thread")]
async fn reauth_clears_sudo() {
    let server = TestServer::start().await;
    let (http, token) = admin_with_token(&server).await;
    http.sudo().await;
    assert!(sudo_active(&server, &token));
    http.admin_post("/reauth", json!({"password": PASSWORD})).await;
    assert!(!sudo_active(&server, &token));
}

// ===================================================================================================
// CSRF-védelem
// ===================================================================================================

#[tokio::test(flavor = "multi_thread")]
async fn a_missing_custom_header_is_refused() {
    let server = TestServer::start().await;
    let http = server.admin_http().await;
    let response = http.request("POST", "/api/admin/sudo", Some(json!({"password": PASSWORD})), &[("Origin", &server.base())]).await;
    assert_eq!(response.status, 403);
    assert!(audit_rows(&server).is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_missing_origin_and_referer_is_refused() {
    let server = TestServer::start().await;
    let http = server.admin_http().await;
    let response = http.request("POST", "/api/admin/sudo", Some(json!({"password": PASSWORD})), &[("X-Admin-Request", "1")]).await;
    assert_eq!(response.status, 403);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_foreign_origin_is_refused() {
    let server = TestServer::start().await;
    let http = server.admin_http().await;
    for origin in ["http://evil.example", "null", "http://127.0.0.1.evil.example".to_string().as_str(), "http://127.0.0.1:9999"] {
        let response = http.request("POST", "/api/admin/sudo", Some(json!({"password": PASSWORD})), &[("X-Admin-Request", "1"), ("Origin", origin)]).await;
        assert_eq!(response.status, 403, "{origin}");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_referer_is_accepted_when_the_origin_is_missing() {
    let server = TestServer::start().await;
    let http = server.admin_http().await;
    let referer = format!("{}/admin", server.base());
    let response = http.request("POST", "/api/admin/sudo", Some(json!({"password": PASSWORD})), &[("X-Admin-Request", "1"), ("Referer", &referer)]).await;
    assert_eq!(response.status, 200);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_foreign_referer_is_refused() {
    let server = TestServer::start().await;
    let http = server.admin_http().await;
    let response =
        http.request("POST", "/api/admin/sudo", Some(json!({"password": PASSWORD})), &[("X-Admin-Request", "1"), ("Referer", "http://evil.example/x")]).await;
    assert_eq!(response.status, 403);
}

#[tokio::test(flavor = "multi_thread")]
async fn get_needs_no_custom_header() {
    let server = TestServer::start().await;
    let http = server.admin_http().await;
    assert_eq!(http.get("/api/admin/audit").await.status, 200);
}

#[tokio::test(flavor = "multi_thread")]
async fn delete_is_protected_too() {
    let server = TestServer::start().await;
    let http = server.admin_http().await;
    assert_eq!(http.delete("/api/admin/sudo").await.status, 403);
    assert_eq!(http.admin_delete("/sudo", json!({})).await.status, 200);
}

// ===================================================================================================
// Sudo mód
// ===================================================================================================

#[tokio::test(flavor = "multi_thread")]
async fn a_wrong_sudo_password() {
    let server = TestServer::start().await;
    let (http, token) = admin_with_token(&server).await;
    let response = http.admin_post("/sudo", json!({"password": "rossz-jelszo"})).await;
    assert_eq!(response.status, 403);
    assert!(!sudo_active(&server, &token));
    let rows = audit_rows(&server);
    assert_eq!(rows.iter().map(|r| r["action"].as_str().unwrap()).collect::<Vec<_>>(), vec!["admin.sudo_failed"]);
    assert!(!rows[0]["ip"].as_str().unwrap_or("").is_empty());
    assert!(rows[0]["admin_user_id"].as_i64().is_some());
}

#[tokio::test(flavor = "multi_thread")]
async fn an_invalid_password_field_is_a_400() {
    let server = TestServer::start().await;
    let http = server.admin_http().await;
    for body in [Some(json!({})), Some(json!({"password": ""})), Some(json!({"password": 12345})), Some(json!({"password": "x".repeat(200)})), None] {
        let response = http.admin("POST", "/sudo", body.clone()).await;
        assert_eq!(response.status, 400, "{body:?}");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn the_right_password_grants_sudo_for_the_configured_time() {
    let server = TestServer::start().await;
    let (http, token) = admin_with_token(&server).await;
    let response = http.admin_post("/sudo", json!({"password": PASSWORD})).await;
    assert_eq!(response.status, 200);
    let remaining = response.json()["session"]["sudo_remaining"].as_i64().unwrap();
    assert!((10 * 60 - 5..=10 * 60).contains(&remaining), "{remaining}");
    assert!(sudo_active(&server, &token));
    let status = http.get("/api/admin/session").await.json()["session"].clone();
    assert!(status["sudo_remaining"].as_i64().unwrap() > 0);
    let rows = audit_rows(&server);
    assert_eq!(rows.iter().map(|r| r["action"].as_str().unwrap()).collect::<Vec<_>>(), vec!["admin.sudo"]);
    assert_eq!(rows[0]["details"]["minutes"], 10);
}

#[tokio::test(flavor = "multi_thread")]
async fn sudo_is_bound_to_the_session() {
    let server = TestServer::start().await;
    let (http, _) = admin_with_token(&server).await;
    http.sudo().await;
    let other = server.http_as(ADMIN_EMAIL).await;
    assert!(!sudo_active(&server, &other.session_token().unwrap()));
}

#[tokio::test(flavor = "multi_thread")]
async fn ending_sudo() {
    let server = TestServer::start().await;
    let (http, token) = admin_with_token(&server).await;
    http.sudo().await;
    let response = http.admin_delete("/sudo", json!({})).await;
    assert_eq!(response.status, 200);
    assert!(!sudo_active(&server, &token));
    assert_eq!(audit_actions(&server), vec!["admin.sudo_end", "admin.sudo"]);
}

#[tokio::test(flavor = "multi_thread")]
async fn wrong_passwords_count_towards_the_login_limit() {
    let server = TestServer::start().await;
    let http = server.admin_http().await;
    restore_default_limits(&server.app);
    let mut statuses = Vec::new();
    for _ in 0..11 {
        statuses.push(http.admin_post("/sudo", json!({"password": "rossz"})).await.status);
    }
    assert_eq!(&statuses[..10], &[403; 10]);
    assert_eq!(statuses[10], 429);
    // A korlát alatt a helyes jelszó sem megy át
    assert_eq!(http.admin_post("/sudo", json!({"password": PASSWORD})).await.status, 429);
}

// ===================================================================================================
// A romboló műveletek őrei
// ===================================================================================================

#[tokio::test(flavor = "multi_thread")]
async fn no_sudo_means_401_sudo_required() {
    let server = TestServer::start().await;
    let (http, _) = admin_with_token(&server).await;
    let response = http.admin_post("/users/1/ban", json!({"reason": "teszt", "duration": "1d"})).await;
    assert_eq!(response.status, 401);
    assert_eq!(response.json()["sudo_required"], true);
}

#[tokio::test(flavor = "multi_thread")]
async fn with_sudo_the_guard_lets_the_request_through() {
    let server = TestServer::start().await;
    let (http, _) = admin_with_token(&server).await;
    http.sudo().await;
    let response = http.admin_post("/users/9999/ban", json!({"reason": "teszt", "duration": "1d"})).await;
    assert_ne!(response.status, 401, "{}", response.text());
}

#[tokio::test(flavor = "multi_thread")]
async fn expired_sudo_is_not_sudo() {
    let server = TestServer::start().await;
    let (http, token) = admin_with_token(&server).await;
    http.sudo().await;
    set_session(&server, &token, "sudo_until", Some("2000-01-01 00:00:00"));
    let response = http.admin_post("/users/1/ban", json!({"reason": "teszt", "duration": "1d"})).await;
    assert_eq!(response.status, 401);
    assert_eq!(response.json()["sudo_required"], true);
}

#[tokio::test(flavor = "multi_thread")]
async fn sudo_does_not_survive_idle_expiry() {
    let server = TestServer::start().await;
    let (http, token) = admin_with_token(&server).await;
    http.sudo().await;
    set_session(&server, &token, "admin_seen_at", Some("2000-01-01 00:00:00"));
    let response = http.admin_post("/users/1/ban", json!({"reason": "teszt", "duration": "1d"})).await;
    assert_eq!(response.status, 401);
    assert_eq!(response.json()["reauth"], true);
}

#[tokio::test(flavor = "multi_thread")]
async fn danger_actions_have_their_own_rate_limit() {
    let server = TestServer::start().await;
    let http = server.admin_http().await;
    restore_default_limits(&server.app);
    let mut statuses = Vec::new();
    // az e-mail küldés a `danger` szintű végpont: a hibás kérés a kezelő előtt is elszámolódik
    for _ in 0..21 {
        statuses.push(http.admin_post("/email", json!({})).await.status);
    }
    assert!(statuses[..20].iter().all(|s| *s == 400), "{statuses:?}");
    assert_eq!(statuses[20], 429);
}

// ===================================================================================================
// Forgalomkorlát
// ===================================================================================================

#[tokio::test(flavor = "multi_thread")]
async fn admin_requests_are_limited_per_ip() {
    let server = TestServer::start().await;
    let http = server.admin_http().await;
    restore_default_limits(&server.app);
    let limit = scrabble::config::AUTH_RATE_LIMITS.iter().find(|(k, _)| *k == "admin").unwrap().1.0 as usize;
    let mut statuses = Vec::new();
    for _ in 0..=limit {
        statuses.push(http.get("/api/admin/session").await.status);
    }
    assert!(statuses[..limit].iter().all(|s| *s == 200), "{statuses:?}");
    assert_eq!(statuses[limit], 429);
}

#[tokio::test(flavor = "multi_thread")]
async fn not_admin_requests_do_not_use_the_admin_budget() {
    let server = TestServer::start().await;
    restore_default_limits(&server.app);
    let http = server.http();
    for _ in 0..130 {
        assert_eq!(http.get("/api/admin/session").await.status, 404);
    }
}

// ===================================================================================================
// IP-engedélylista
// ===================================================================================================

#[test]
fn the_allowlist_parsing() {
    assert_eq!(parse_ip_networks("10.0.0.0/8, 192.168.1.5 ,::1").unwrap().len(), 3);
    assert!(parse_ip_networks("").unwrap().is_empty());
}

#[test]
fn an_invalid_allowlist_entry_fails_loudly() {
    let error = parse_ip_networks("10.0.0.0/8, nem-ip").unwrap_err();
    assert!(error.contains("ADMIN_IP_ALLOWLIST"), "{error}");
}

#[test]
fn ip_allowed_matches_networks_hosts_and_mapped_addresses() {
    let networks = parse_ip_networks("10.0.0.0/8,192.168.1.5,fd00::/8").unwrap();
    assert!(admin::ip_allowed("10.20.30.40", &networks));
    assert!(admin::ip_allowed("192.168.1.5", &networks));
    assert!(admin::ip_allowed("fd00::1", &networks));
    assert!(admin::ip_allowed("::ffff:10.0.0.1", &networks));
    assert!(!admin::ip_allowed("192.168.1.6", &networks));
    assert!(!admin::ip_allowed("11.0.0.1", &networks));
    assert!(!admin::ip_allowed("nem-ip", &networks));
    assert!(!admin::ip_allowed("", &networks));
}

#[test]
fn an_empty_list_allows_everyone() {
    assert!(admin::ip_allowed("203.0.113.9", &[]));
}

#[tokio::test(flavor = "multi_thread")]
async fn other_ips_get_a_404_even_as_admin() {
    let server = TestServer::start_with(|c| c.admin_ip_allowlist = parse_ip_networks("10.0.0.0/8").unwrap()).await;
    let reference = reference_404(&server).await;
    let http = server.admin_http().await;
    // a tesztkliens a loopbackről jön, ezért az átküldött (tunnel) cím számít
    http.add_header("CF-Connecting-IP", "10.1.2.3");
    assert_eq!(http.get("/api/admin/session").await.status, 200);
    let blocked = server.http();
    blocked.set_cookie(http.session_token().as_deref());
    blocked.add_header("CF-Connecting-IP", "203.0.113.9");
    for path in ["/api/admin/session", "/admin", "/admin/assets/admin.js"] {
        assert_plain_404(&blocked.get(path).await, &reference, path);
    }
}

#[test]
fn the_forwarded_header_is_ignored_from_a_non_local_peer() {
    use axum::http::{HeaderMap, HeaderValue};
    let mut headers = HeaderMap::new();
    headers.insert("cf-connecting-ip", HeaderValue::from_static("10.1.2.3"));
    let remote: std::net::IpAddr = "203.0.113.9".parse().unwrap();
    assert_eq!(scrabble::server::net::client_ip(&headers, Some(remote)), "203.0.113.9");
}

#[test]
fn a_tunnel_peer_uses_the_forwarded_address() {
    use axum::http::{HeaderMap, HeaderValue};
    let mut headers = HeaderMap::new();
    headers.insert("cf-connecting-ip", HeaderValue::from_static("10.1.2.3"));
    let local: std::net::IpAddr = "127.0.0.1".parse().unwrap();
    assert_eq!(scrabble::server::net::client_ip(&headers, Some(local)), "10.1.2.3");
    assert_eq!(scrabble::server::net::client_ip(&HeaderMap::new(), Some(local)), "127.0.0.1");
}

// ===================================================================================================
// Socket.IO
// ===================================================================================================

fn event_names(events: &[(String, Value)]) -> Vec<String> {
    events.iter().map(|(n, _)| n.clone()).collect()
}

#[tokio::test(flavor = "multi_thread")]
async fn a_guest_gets_nothing() {
    let server = TestServer::start().await;
    let guest = server.guest("Vendég").await;
    assert!(event_names(&guest.call("admin_subscribe", Value::Null).await).is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_non_admin_gets_nothing() {
    let server = TestServer::start().await;
    let id = server.create_user("player@example.com", "Játékos");
    let client = sio_user(&server, id, "Játékos").await;
    assert!(event_names(&client.call("admin_subscribe", Value::Null).await).is_empty());
    server.app.emit_room("admin", "admin_secret", &json!({}));
    client.settle().await;
    assert!(event_names(&client.take()).is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn an_admin_is_subscribed() {
    let server = TestServer::start().await;
    let id = server.create_user(ADMIN_EMAIL, "Főnök");
    let client = sio_user(&server, id, "Főnök").await;
    assert_eq!(event_names(&client.call("admin_subscribe", Value::Null).await), vec!["admin_subscribed", "admin_overview"]);
    server.app.emit_room("admin", "admin_overview", &json!({"x": 1}));
    client.settle().await;
    assert_eq!(event_names(&client.take()), vec!["admin_overview"]);
}

#[tokio::test(flavor = "multi_thread")]
async fn an_empty_admin_list_means_no_subscription() {
    let server = TestServer::start_with(|c| c.admin_emails.clear()).await;
    let id = server.create_user(ADMIN_EMAIL, "Főnök");
    let client = sio_user(&server, id, "Főnök").await;
    assert!(event_names(&client.call("admin_subscribe", Value::Null).await).is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn logging_out_leaves_the_admin_room() {
    let server = TestServer::start().await;
    let id = server.create_user(ADMIN_EMAIL, "Főnök");
    let client = sio_user(&server, id, "Főnök").await;
    client.call("admin_subscribe", Value::Null).await;
    client.call("logout", Value::Null).await;
    server.app.emit_room("admin", "admin_overview", &json!({}));
    client.settle().await;
    assert!(event_names(&client.take()).is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn switching_to_a_guest_identity_leaves_the_admin_room() {
    let server = TestServer::start().await;
    let id = server.create_user(ADMIN_EMAIL, "Főnök");
    let client = sio_user(&server, id, "Főnök").await;
    client.call("admin_subscribe", Value::Null).await;
    client.call("set_name", json!({"name": "Vendég", "is_guest": true})).await;
    server.app.emit_room("admin", "admin_overview", &json!({}));
    client.settle().await;
    assert!(event_names(&client.take()).is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn subscribing_is_rate_limited_silently() {
    let server = TestServer::start().await;
    let id = server.create_user(ADMIN_EMAIL, "Főnök");
    let client = sio_user(&server, id, "Főnök").await;
    for _ in 0..5 {
        client.emit("admin_subscribe", Value::Null);
    }
    client.settle_long().await;
    client.take();
    assert!(event_names(&client.call("admin_subscribe", Value::Null).await).is_empty());
}

// ===================================================================================================
// A service worker nem érinti az admint
// ===================================================================================================

const SW_HARNESS: &str = r#"
const vm = require('vm');
const code = require('fs').readFileSync(0, 'utf8');
const listeners = {};
const sandbox = {
    self: { addEventListener: (type, fn) => { listeners[type] = fn; }, location: { origin: 'https://example.test' },
            skipWaiting() {}, clients: { claim() {} } },
    caches: { match: async () => undefined, keys: async () => [],
              open: async () => ({ match: async () => undefined, put() {}, addAll: async () => {} }) },
    fetch: async () => ({ ok: true, clone() { return this; } }),
    URL, console,
};
vm.createContext(sandbox);
vm.runInContext(code, sandbox);
function handled(path, mode) {
    let called = false;
    listeners.fetch({ request: { method: 'GET', url: 'https://example.test' + path, mode },
                      respondWith() { called = true; } });
    return called;
}
console.log(JSON.stringify({
    admin_page: handled('/admin', 'navigate'),
    admin_asset: handled('/admin/assets/admin.js', 'no-cors'),
    admin_api: handled('/api/admin/session', 'cors'),
    public_page: handled('/', 'navigate'),
    public_static: handled('/static/app.js', 'no-cors'),
}));
"#;

fn node_available() -> bool {
    std::process::Command::new("node").arg("--version").output().map(|o| o.status.success()).unwrap_or(false)
}

#[tokio::test(flavor = "multi_thread")]
async fn admin_requests_bypass_the_service_worker() {
    if !node_available() {
        eprintln!("kimarad: a node nem elérhető");
        return;
    }
    use std::io::Write;
    let server = TestServer::start().await;
    let source = server.http().get("/sw.js").await.text();
    let mut child = std::process::Command::new("node")
        .args(["-e", SW_HARNESS])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(source.as_bytes()).unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let handled: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(handled, json!({"admin_page": false, "admin_asset": false, "admin_api": false, "public_page": true, "public_static": true}));
}

#[tokio::test(flavor = "multi_thread")]
async fn admin_is_not_part_of_the_precached_shell() {
    let server = TestServer::start().await;
    let source = server.http().get("/sw.js").await.text();
    let shell = regex::Regex::new(r"(?s)const SHELL_URLS = \[(.*?)\];").unwrap().captures(&source).expect("SHELL_URLS")[1].to_string();
    assert!(!shell.contains("admin"), "{shell}");
}
