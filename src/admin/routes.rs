//! Admin panel HTTP réteg: őr, munkamenet (újraigazolás, sudo), napló és a panel kiszolgálása (a Python
//! `admin_routes.py` portja).
//!
//! Az admin felület létezése másnak nem derülhet ki: nem admin kérésre MINDEN admin útvonal (oldal, asset, API, rossz
//! metódus, nem létező alútvonal) ugyanazt a szokásos 404-et adja, mint egy nem létező cím. Az őr ezért az egész
//! alkalmazás elé kerül, és az útvonal-illesztés előtt dönt. A logika az `admin` modulokban van; részletes leírás:
//! `docs/ADMIN_PANEL.md`.

use super::audit::{AuditFilters, audit_csv, query_audit};
use super::session::{end_sudo, grant_sudo, reauth, record_failed_password, session_status, sudo_active, touch};
use super::{AUDIT_EXPORT_MAX, AdminContext, AdminError, AdminResult, record};
use crate::app::App;
use crate::server::http::{self, Client, fail, json_object, not_found};
use axum::Router;
use axum::extract::{RawPathParams, Request, State};
use axum::http::{HeaderMap, HeaderValue, Method, StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::routing::{MethodRouter, delete, get, patch, post, put};
use serde_json::{Map, Value, json};
use std::collections::HashMap;
use std::sync::Arc;
use tower_http::services::ServeDir;

const API_PREFIX: &str = "/api/admin";
const PAGE_PREFIX: &str = "/admin";

/// A Socket.IO kliens ugyanonnan töltődik, mint a nyilvános oldalon (cdnjs); a WebSocket kapcsolat a saját hosttal
const CSP: &str = "default-src 'none'; script-src 'self' https://cdnjs.cloudflare.com; style-src 'self'; img-src 'self' data:; \
                   font-src 'self'; connect-src 'self' ws: wss:; base-uri 'none'; form-action 'none'; frame-ancestors 'none'";

// ===================================================================================================
// Kérés / válasz segédek
// ===================================================================================================

#[derive(Clone, Debug)]
pub struct AdminUser {
    pub id: i64,
    pub email: String,
    pub display_name: String,
}

/// Az őr által azonosított admin munkamenet (a kérés kiterjesztésében utazik a kezelőig).
#[derive(Clone, Debug)]
pub struct AdminSession {
    pub user: AdminUser,
    pub token: String,
    pub ctx: AdminContext,
}

/// Az admin kezelők bemenete.
pub struct AdminReq {
    pub app: Arc<App>,
    pub admin: AdminSession,
    pub method: Method,
    pub query: HashMap<String, String>,
    /// a JSON törzs szótárként (különben üres szótár)
    pub body: Value,
    pub params: HashMap<String, String>,
}

pub type HResult = AdminResult<Response>;
pub type Handler = fn(&AdminReq) -> HResult;

impl AdminReq {
    pub fn ctx(&self) -> &AdminContext {
        &self.admin.ctx
    }

    pub fn arg(&self, key: &str) -> Option<&str> {
        self.query.get(key).map(|v| v.as_str())
    }

    /// `request.args.get(kulcs, alapérték, type=int)`: hibás szám → az alapérték.
    pub fn int_arg(&self, key: &str, default: i64) -> i64 {
        self.arg(key).and_then(|v| v.trim().parse().ok()).unwrap_or(default)
    }

    /// Az indoklás a kérés törzséből (a hiányát / rövidségét az `admin::action` jelzi).
    pub fn reason(&self) -> Option<&Value> {
        self.body.get("reason")
    }

    pub fn field(&self, key: &str) -> Option<&Value> {
        self.body.get(key)
    }

    /// Egész szám útvonal-paraméter; hibás érték → 404 (mint a Flask `<int:...>` konvertere).
    pub fn id(&self, name: &str) -> AdminResult<i64> {
        self.params.get(name).and_then(|v| v.parse::<i64>().ok()).ok_or_else(|| AdminError::new("Nem található.", 404))
    }

    pub fn param(&self, name: &str) -> &str {
        self.params.get(name).map(|v| v.as_str()).unwrap_or("")
    }

    pub fn wants_csv(&self) -> bool {
        self.arg("format") == Some("csv")
    }

    pub fn sudo_active(&self) -> bool {
        sudo_active(&self.app, &self.admin.token)
    }

    /// Feltételes sudo-igény: a 401 `sudo_required` hibát adja, ha nincs érvényes megerősítés.
    pub fn require_sudo(&self) -> AdminResult<()> {
        if self.sudo_active() { Ok(()) } else { Err(sudo_error()) }
    }
}

pub fn sudo_error() -> AdminError {
    AdminError::new("Ehhez a művelethez jelszó-megerősítés (sudo) szükséges.", 401).with("sudo_required", json!(true))
}

/// `{success: true, ...mezők}` válasz (a Flask `json_ok(**adatok)`).
pub fn json_ok(data: Value) -> HResult {
    Ok(http::success_with(data))
}

pub fn json_ok_empty() -> HResult {
    Ok(http::ok(json!({"success": true})))
}

fn error_response(error: AdminError) -> Response {
    let mut body = Map::new();
    body.insert("success".into(), json!(false));
    body.insert("message".into(), json!(error.message));
    for (k, v) in error.extra {
        body.insert(k, v);
    }
    http::json_response(error.status, Value::Object(body))
}

/// CSV letöltés (UTF-8, a képletként értelmezhető cellák elé `'` kerül).
pub fn csv_response(filename: &str, fields: &[&str], rows: &[Value]) -> Response {
    let mut out = super::csv_line(&fields.iter().map(|f| f.to_string()).collect::<Vec<_>>());
    for row in rows {
        let cells: Vec<String> = fields.iter().map(|f| super::csv_cell(&flat(row.get(*f).unwrap_or(&Value::Null)))).collect();
        out.push_str(&super::csv_line(&cells));
    }
    csv_text_response(filename, out)
}

pub fn csv_text_response(filename: &str, text: String) -> Response {
    let mut response = (StatusCode::OK, text).into_response();
    let headers = response.headers_mut();
    headers.insert(header::CONTENT_TYPE, HeaderValue::from_static("text/csv; charset=utf-8"));
    if let Ok(value) = HeaderValue::from_str(&format!("attachment; filename=\"{filename}\"")) {
        headers.insert(header::CONTENT_DISPOSITION, value);
    }
    response
}

/// JSON letöltés fájlként.
pub fn json_download(filename: &str, text: String) -> Response {
    let mut response = (StatusCode::OK, text).into_response();
    let headers = response.headers_mut();
    headers.insert(header::CONTENT_TYPE, HeaderValue::from_static("application/json; charset=utf-8"));
    if let Ok(value) = HeaderValue::from_str(&format!("attachment; filename=\"{filename}\"")) {
        headers.insert(header::CONTENT_DISPOSITION, value);
    }
    response
}

/// Lista / szótár cellák CSV-hez: egyszerű szöveggé alakítva.
pub fn flat(value: &Value) -> Value {
    match value {
        Value::Array(list) => json!(list.iter().map(|v| flat_text(&flat(v))).collect::<Vec<_>>().join("; ")),
        Value::Object(map) => json!(map.iter().map(|(k, v)| format!("{k}={}", flat_text(&flat(v)))).collect::<Vec<_>>().join("; ")),
        other => other.clone(),
    }
}

fn flat_text(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        Value::Null => "None".to_string(),
        other => other.to_string(),
    }
}

// ===================================================================================================
// Őr
// ===================================================================================================

/// 'api' / 'page' az admin útvonalaknál, különben None. A dupla perjel (//admin) összevonva.
fn admin_path_kind(path: &str) -> Option<&'static str> {
    let mut collapsed = String::with_capacity(path.len());
    let mut previous_slash = false;
    for c in path.chars() {
        if c == '/' {
            if !previous_slash {
                collapsed.push(c);
            }
            previous_slash = true;
        } else {
            collapsed.push(c);
            previous_slash = false;
        }
    }
    for (prefix, kind) in [(API_PREFIX, "api"), (PAGE_PREFIX, "page")] {
        if collapsed == prefix || collapsed.starts_with(&format!("{prefix}/")) {
            return Some(kind);
        }
    }
    None
}

/// Az Origin / Referer fejléc hostja (a `host[:port]` rész).
fn netloc_of(url: &str) -> Option<String> {
    let rest = url.split_once("://").map(|(_, r)| r)?;
    let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let netloc = &rest[..end];
    let netloc = netloc.rsplit_once('@').map(|(_, h)| h).unwrap_or(netloc);
    if netloc.is_empty() { None } else { Some(netloc.to_lowercase()) }
}

/// A kérés Origin (vagy Referer) fejléce a saját hostra mutat-e (a SameSite=Lax mellé).
fn same_origin(headers: &HeaderMap) -> bool {
    let source = headers.get(header::ORIGIN).or_else(|| headers.get(header::REFERER)).and_then(|v| v.to_str().ok()).filter(|s| !s.is_empty());
    let host = headers.get(header::HOST).and_then(|v| v.to_str().ok()).unwrap_or("").to_lowercase();
    match source.and_then(netloc_of) {
        Some(netloc) => netloc == host,
        None => false,
    }
}

fn is_unsafe(method: &Method) -> bool {
    matches!(*method, Method::POST | Method::PUT | Method::PATCH | Method::DELETE)
}

fn collapse_slashes(path: &str) -> String {
    let mut out = String::with_capacity(path.len());
    let mut previous = false;
    for c in path.chars() {
        if c == '/' && previous {
            continue;
        }
        previous = c == '/';
        out.push(c);
    }
    out
}

/// Ezek a végpontok tétlenség után is működnek (a panel ezekkel kéri a jelszót), és nem frissítik az időzítőt.
fn idle_exempt(method: &Method, path: &str) -> bool {
    let path = collapse_slashes(path);
    (*method == Method::GET && (path == "/api/admin/session" || path == "/admin" || path.starts_with("/admin/assets/")))
        || (*method == Method::POST && path == "/api/admin/reauth")
}

fn admin_headers(response: &mut Response) {
    let headers = response.headers_mut();
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    headers.insert("x-robots-tag", HeaderValue::from_static("noindex, nofollow"));
    headers.insert(header::X_FRAME_OPTIONS, HeaderValue::from_static("DENY"));
    headers.insert(header::X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
    headers.insert(header::REFERRER_POLICY, HeaderValue::from_static("no-referrer"));
    headers.insert(header::CONTENT_SECURITY_POLICY, HeaderValue::from_static(CSP));
}

/// Az őr: nem admin útvonalnál átengedi a kérést, adminnál a lépéseket sorban végzi (azonosítás → IP-lista →
/// forgalomkorlát → CSRF → tétlenség).
pub async fn guard(app: &Arc<App>, mut request: Request, next: Next) -> Response {
    if admin_path_kind(request.uri().path()).is_none() {
        return next.run(request).await;
    }
    // 1. Azonosítás: nem admin → ugyanaz a 404, mint egy nem létező útvonalra
    if app.config.admin_emails.is_empty() {
        return not_found();
    }
    let client = Client::from_request(&request);
    let user = client.user(app).filter(|u| app.is_admin_email(&u.email));
    let (Some(user), Some(token)) = (user, client.token.clone()) else { return not_found() };
    // 2. IP-engedélylista (ha be van állítva) — kívülről szintén 404
    if !super::ip_allowed(&client.ip, &app.config.admin_ip_allowlist) {
        return not_found();
    }
    let session = AdminSession {
        user: AdminUser { id: user.id, email: user.email.clone(), display_name: user.display_name.clone() },
        token: token.clone(),
        ctx: AdminContext { admin_user_id: user.id, ip: client.ip.clone(), user_agent: client.user_agent.clone() },
    };
    let (method, path, headers) = (request.method().clone(), request.uri().path().to_string(), request.headers().clone());
    let mut response = match check_request(app, &method, &path, &headers, &session).await {
        Some(early) => early,
        None => {
            request.extensions_mut().insert(session);
            next.run(request).await
        }
    };
    admin_headers(&mut response);
    response
}

/// Az őr 3–5. lépése; hiba esetén a kész válasz.
async fn check_request(app: &Arc<App>, method: &Method, path: &str, headers: &HeaderMap, session: &AdminSession) -> Option<Response> {
    // 3. Forgalomkorlát
    if !app.limiter.check_ip(&session.ctx.ip, "admin") {
        return Some(error_response(AdminError::new("Túl sok kérés. Várj egy kicsit.", 429)));
    }
    // 4. Más oldalról indított kérések ellen: egyedi fejléc + azonos eredetű Origin / Referer
    if is_unsafe(method) {
        let marker = headers.get("x-admin-request").and_then(|v| v.to_str().ok());
        if marker != Some("1") || !same_origin(headers) {
            return Some(error_response(AdminError::new("Érvénytelen kérés.", 403)));
        }
    }
    // 5. Tétlenségi időkorlát: lejárt munkamenetnél jelszó kell (az oldal és az újraigazolás kivétel)
    if idle_exempt(method, path) {
        return None;
    }
    let app = app.clone();
    let token = session.token.clone();
    let status = tokio::task::spawn_blocking(move || {
        let status = session_status(&app, &token);
        if let Some(status) = status.as_ref().filter(|s| !s.idle_expired()) {
            touch(&app, &token, Some(status));
        }
        status
    })
    .await
    .ok()
    .flatten();
    match status {
        Some(status) if !status.idle_expired() => None,
        _ => Some(error_response(AdminError::new("A munkamenet lejárt, add meg újra a jelszavad.", 401).with("reauth", json!(true)))),
    }
}

// ===================================================================================================
// Útvonal-nyilvántartás
// ===================================================================================================

/// A védelem szintje: sima, romboló (külön forgalomkorlát) vagy sudo-hoz kötött.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Level {
    Normal,
    Danger,
    Sudo,
}

/// Az admin API útvonalai: az axum útválasztó és (a hozzáférési tesztekhez) az útvonaltérkép.
pub struct AdminRouter {
    router: Router<Arc<App>>,
    pub table: Vec<(Method, String)>,
}

impl AdminRouter {
    fn new() -> AdminRouter {
        AdminRouter { router: Router::new(), table: Vec::new() }
    }

    pub fn route(&mut self, method: Method, path: &str, level: Level, handler: Handler) {
        let full = format!("{API_PREFIX}{path}");
        let h = move |State(app): State<Arc<App>>, params: RawPathParams, request: Request| {
            let params: HashMap<String, String> = params.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
            dispatch(app, params, request, handler, level)
        };
        let router: MethodRouter<Arc<App>> = match method {
            Method::GET => get(h),
            Method::POST => post(h),
            Method::PATCH => patch(h),
            Method::PUT => put(h),
            Method::DELETE => delete(h),
            _ => return,
        };
        self.router = std::mem::take(&mut self.router).route(&full, router);
        self.table.push((method, full));
    }

    pub fn get(&mut self, path: &str, handler: Handler) {
        self.route(Method::GET, path, Level::Normal, handler);
    }
    pub fn post(&mut self, path: &str, handler: Handler) {
        self.route(Method::POST, path, Level::Normal, handler);
    }
    pub fn patch(&mut self, path: &str, handler: Handler) {
        self.route(Method::PATCH, path, Level::Normal, handler);
    }
    pub fn delete(&mut self, path: &str, handler: Handler) {
        self.route(Method::DELETE, path, Level::Normal, handler);
    }
    /// Romboló művelet: külön, szigorúbb forgalomkorlát (`admin_danger`).
    pub fn post_danger(&mut self, path: &str, handler: Handler) {
        self.route(Method::POST, path, Level::Danger, handler);
    }
    pub fn delete_danger(&mut self, path: &str, handler: Handler) {
        self.route(Method::DELETE, path, Level::Danger, handler);
    }
    pub fn patch_danger(&mut self, path: &str, handler: Handler) {
        self.route(Method::PATCH, path, Level::Danger, handler);
    }
    /// Romboló művelet, amelyhez friss jelszó-megerősítés (sudo mód) is kell.
    pub fn post_sudo(&mut self, path: &str, handler: Handler) {
        self.route(Method::POST, path, Level::Sudo, handler);
    }
    pub fn get_sudo(&mut self, path: &str, handler: Handler) {
        self.route(Method::GET, path, Level::Sudo, handler);
    }
    pub fn patch_sudo(&mut self, path: &str, handler: Handler) {
        self.route(Method::PATCH, path, Level::Sudo, handler);
    }
    pub fn delete_sudo(&mut self, path: &str, handler: Handler) {
        self.route(Method::DELETE, path, Level::Sudo, handler);
    }
}

fn parse_query(raw: Option<&str>) -> HashMap<String, String> {
    raw.and_then(|q| serde_urlencoded::from_str::<Vec<(String, String)>>(q).ok())
        .map(|pairs| {
            let mut map = HashMap::new();
            for (k, v) in pairs {
                map.entry(k).or_insert(v); // a Flask `args.get` az első értéket adja
            }
            map
        })
        .unwrap_or_default()
}

async fn dispatch(app: Arc<App>, params: HashMap<String, String>, request: Request, handler: Handler, level: Level) -> Response {
    let Some(admin) = request.extensions().get::<AdminSession>().cloned() else { return not_found() };
    let method = request.method().clone();
    let query = parse_query(request.uri().query());
    let headers = request.headers().clone();
    let bytes = axum::body::to_bytes(request.into_body(), 8_000_000).await.unwrap_or_default();
    let body = json_object(&headers, &bytes);
    let req = AdminReq { app, admin, method, query, body, params };
    let result = tokio::task::spawn_blocking(move || run(&req, handler, level)).await;
    match result {
        Ok(response) => response,
        Err(e) => {
            println!("[admin] A kezelő hibával leállt: {e}");
            fail(500, "Belső hiba.")
        }
    }
}

fn run(req: &AdminReq, handler: Handler, level: Level) -> Response {
    if level != Level::Normal && !req.app.limiter.check_ip(&req.admin.ctx.ip, "admin_danger") {
        return error_response(AdminError::new("Túl sok kérés. Várj egy kicsit.", 429));
    }
    if level == Level::Sudo && !req.sudo_active() {
        return error_response(sudo_error());
    }
    match handler(req) {
        Ok(response) => response,
        Err(error) => error_response(error),
    }
}

// ===================================================================================================
// Munkamenet és napló végpontok
// ===================================================================================================

fn session_payload(req: &AdminReq) -> Value {
    let status = session_status(&req.app, &req.admin.token);
    json!({
        "success": true,
        "admin": {"id": req.admin.user.id, "display_name": req.admin.user.display_name, "email": req.admin.user.email},
        "session": status.map(|s| s.as_json()),
    })
}

/// Az admin jelszavának ellenőrzése (sudo / újraigazolás). Hiba esetén a kész válasz. A rossz próbálkozások a
/// bejelentkezés forgalomkorlátjába számítanak, és naplózódnak.
fn check_password(req: &AdminReq, failed_action: &str) -> AdminResult<()> {
    let ctx = req.ctx();
    if !req.app.limiter.check_ip(&ctx.ip, "login") {
        return Err(AdminError::new("Túl sok jelszópróbálkozás. Próbáld újra 5 perc múlva.", 429));
    }
    let password = req.field("password").and_then(|p| p.as_str()).filter(|p| !p.is_empty() && p.chars().count() <= 128);
    let Some(password) = password else {
        return Err(AdminError::new("A jelszó megadása kötelező.", 400));
    };
    if req.app.db.verify_password(&req.admin.user.email, password).is_err() {
        record_failed_password(&req.app, ctx, failed_action)?;
        return Err(AdminError::new("Hibás jelszó.", 403));
    }
    Ok(())
}

fn session_info(req: &AdminReq) -> HResult {
    Ok(http::ok(session_payload(req)))
}

fn reauth_handler(req: &AdminReq) -> HResult {
    check_password(req, "admin.reauth_failed")?;
    reauth(&req.app, req.ctx(), &req.admin.token)?;
    Ok(http::ok(session_payload(req)))
}

fn sudo_start(req: &AdminReq) -> HResult {
    check_password(req, "admin.sudo_failed")?;
    grant_sudo(&req.app, req.ctx(), &req.admin.token)?;
    Ok(http::ok(session_payload(req)))
}

fn sudo_end(req: &AdminReq) -> HResult {
    end_sudo(&req.app, req.ctx(), &req.admin.token)?;
    Ok(http::ok(session_payload(req)))
}

/// A napló szűrve és lapozva. `format=csv` vagy `download=1` az exportot adja (az export maga is naplózódik).
fn audit_list(req: &AdminReq) -> HResult {
    let filters = AuditFilters::from_args(&req.query);
    let format = req.arg("format").unwrap_or("json");
    if format != "json" && format != "csv" {
        return Err(AdminError::new("Ismeretlen formátum.", 400));
    }
    let export = format == "csv" || req.arg("download") == Some("1");
    if export {
        let limit = req.arg("limit").map(|l| l.to_string()).unwrap_or_else(|| AUDIT_EXPORT_MAX.to_string());
        let result = query_audit(&req.app.db, &filters, Some(&limit), None, AUDIT_EXPORT_MAX)?;
        let rows = result["items"].as_array().map(|a| a.len()).unwrap_or(0);
        req.app.db.with(|tx| {
            record(tx, req.ctx(), "view.audit_export", Some("audit"), None, &json!({"format": format, "filters": filters.active(), "rows": rows})).map(|_| ())
        })?;
        let items = result["items"].as_array().cloned().unwrap_or_default();
        if format == "csv" {
            return Ok(csv_text_response("admin-audit.csv", audit_csv(&items)));
        }
        let mut payload = json!({"success": true});
        if let Value::Object(map) = &result {
            for (k, v) in map {
                payload[k] = v.clone();
            }
        }
        return Ok(json_download("admin-audit.json", payload.to_string()));
    }
    let result = query_audit(&req.app.db, &filters, req.arg("limit"), req.arg("offset"), super::AUDIT_MAX_LIMIT)?;
    json_ok(result)
}

// ===================================================================================================
// A panel oldala és assetjei
// ===================================================================================================

async fn admin_page(State(app): State<Arc<App>>) -> Response {
    match http::render_admin_page(&app) {
        Some(body) => http::html_response(body),
        None => not_found(),
    }
}

/// Az admin útvonalak: a panel oldala, az assetek és az API.
pub fn admin_router(app: &Arc<App>) -> (Router<Arc<App>>, Vec<(Method, String)>) {
    let mut r = AdminRouter::new();
    r.get("/session", session_info);
    r.post("/reauth", reauth_handler);
    r.post("/sudo", sudo_start);
    r.delete("/sudo", sudo_end);
    r.get("/audit", audit_list);
    super::api::register_all(&mut r);

    let AdminRouter { router, mut table } = r;
    let assets = ServeDir::new(app.base_dir.join("web/admin"));
    let router = router.route("/admin", get(admin_page)).nest_service("/admin/assets", assets);
    table.push((Method::GET, "/admin".to_string()));
    (router, table)
}

/// Az admin útvonalak (metódus, útvonal) listája — a hozzáférési tesztekhez.
pub fn route_table(app: &Arc<App>) -> Vec<(Method, String)> {
    admin_router(app).1
}
