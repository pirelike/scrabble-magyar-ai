//! HTTP útvonalak (a Python `routes.py` portja): főoldal és PWA, hitelesítés, nyilvános API (ranglista, napi
//! feladvány, gyakorló módok, szótár), játék API (visszajátszás, elemzés, levelezős lista).

use super::core::{emit_rooms_list, sanitize_name_str};
use super::net::client_ip;
use crate::admin::{comm, moderation};
use crate::ai;
use crate::analysis;
use crate::app::{AnalysisJob, App};
use crate::daily;
use crate::db::{self, LEADERBOARD_METRICS, RowExt};
use crate::dictionary;
use crate::practice;
use crate::push;
use crate::socket_auth::create_socket_token;
use crate::tiles::{self, tokenize_word};
use crate::util;
use crate::word_review;
use axum::Router;
use axum::body::Bytes;
use axum::extract::{ConnectInfo, FromRequestParts, Path, Query, State};
use axum::http::request::Parts;
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use once_cell::sync::Lazy;
use regex::Regex;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;

pub const SESSION_COOKIE: &str = "session_token";
const MAX_DICT_WORDS: usize = 8;
const MAX_SUGGESTION_WORDS: usize = 3;
const ANALYSIS_RETRY_AFTER: f64 = 30.0;

static EMAIL_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"^[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\.[a-zA-Z]{2,}$").expect("érvényes regex"));
static SHARE_TOKEN_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"^[A-Za-z0-9_-]{6,40}$").expect("érvényes regex"));
static PUSH_ENDPOINT_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"^https://\S{10,1000}$").expect("érvényes regex"));

// ===================================================================================================
// Segédek
// ===================================================================================================

/// A kérés kliens-adatai: IP, fejlécek, munkamenet-süti.
pub struct Client {
    pub ip: String,
    pub headers: HeaderMap,
    pub token: Option<String>,
    pub user_agent: String,
    /// a kapcsolat (a proxy fejléce szerint) HTTPS
    pub secure: bool,
}

impl<S: Send + Sync> FromRequestParts<S> for Client {
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let peer: Option<IpAddr> = parts.extensions.get::<ConnectInfo<SocketAddr>>().map(|c| c.0.ip());
        Ok(Client::from_parts(&parts.headers, peer))
    }
}

impl Client {
    /// A kliens-adatok egy teljes kérésből (az őr a kérés kiterjesztéséből veszi a kapcsolat címét).
    pub fn from_request(request: &axum::extract::Request) -> Client {
        let peer: Option<IpAddr> = request.extensions().get::<ConnectInfo<SocketAddr>>().map(|c| c.0.ip());
        Client::from_parts(request.headers(), peer)
    }

    pub fn from_parts(headers: &HeaderMap, peer: Option<IpAddr>) -> Client {
        let ip = client_ip(headers, peer);
        let from_proxy = peer.is_none_or(|p| match p {
            IpAddr::V6(v6) => v6.to_ipv4_mapped().map(IpAddr::V4).unwrap_or(p).is_loopback(),
            other => other.is_loopback(),
        });
        let secure = from_proxy && headers.get("x-forwarded-proto").and_then(|v| v.to_str().ok()).is_some_and(|p| p.eq_ignore_ascii_case("https"));
        Client {
            ip,
            token: cookie(headers, SESSION_COOKIE),
            user_agent: headers.get(header::USER_AGENT).and_then(|v| v.to_str().ok()).unwrap_or("").to_string(),
            headers: headers.clone(),
            secure,
        }
    }

    /// A munkamenethez tartozó felhasználó (érvényes, nem kitiltott munkamenet).
    pub fn user(&self, app: &App) -> Option<db::SessionUser> {
        app.db.validate_session(self.token.as_deref())
    }
}

/// Egy süti értéke a `Cookie` fejlécből.
pub fn cookie(headers: &HeaderMap, name: &str) -> Option<String> {
    for value in headers.get_all(header::COOKIE) {
        for pair in value.to_str().ok()?.split(';') {
            let (key, val) = pair.trim().split_once('=')?;
            if key == name {
                return Some(val.to_string());
            }
        }
    }
    None
}

pub fn json_response(status: u16, value: Value) -> Response {
    (StatusCode::from_u16(status).unwrap_or(StatusCode::OK), axum::Json(value)).into_response()
}

pub fn ok(value: Value) -> Response {
    json_response(200, value)
}

/// `{success: false, message}` hibaválasz.
pub fn fail(status: u16, message: &str) -> Response {
    json_response(status, json!({"success": false, "message": message}))
}

/// Egy `{success: true, ...extra}` válasz.
pub fn success_with(extra: Value) -> Response {
    let mut payload = json!({"success": true});
    if let (Some(base), Some(more)) = (payload.as_object_mut(), extra.as_object()) {
        for (k, v) in more {
            base.insert(k.clone(), v.clone());
        }
    }
    ok(payload)
}

/// A kérés JSON törzse (a Flask `get_json(silent=True)` megfelelője: nem JSON tartalomtípus vagy hibás törzs → None).
pub fn json_body(headers: &HeaderMap, body: &Bytes) -> Option<Value> {
    let content_type = headers.get(header::CONTENT_TYPE).and_then(|v| v.to_str().ok()).unwrap_or("").to_ascii_lowercase();
    let mime = content_type.split(';').next().unwrap_or("").trim().to_string();
    if !(mime == "application/json" || mime.ends_with("+json")) {
        return None;
    }
    serde_json::from_slice(body).ok()
}

/// A JSON törzs szótárként (különben üres szótár), mint a Python `request.get_json(silent=True) or {}`.
pub fn json_object(headers: &HeaderMap, body: &Bytes) -> Value {
    match json_body(headers, body) {
        Some(value @ Value::Object(_)) => value,
        _ => json!({}),
    }
}

/// Egy szöveges mező a törzsből (nem szöveg → ''), levágva.
fn str_field(data: &Value, key: &str) -> String {
    data.get(key).and_then(|v| v.as_str()).map(|s| s.trim().to_string()).unwrap_or_default()
}

fn set_cookie_header(token: &str, secure: bool) -> HeaderValue {
    let mut text = format!("{SESSION_COOKIE}={token}; HttpOnly; SameSite=Lax; Max-Age={}; Path=/", 30 * 24 * 3600);
    if secure {
        text.push_str("; Secure");
    }
    HeaderValue::from_str(&text).expect("érvényes süti")
}

pub fn with_session_cookie(mut response: Response, token: &str, secure: bool) -> Response {
    response.headers_mut().append(header::SET_COOKIE, set_cookie_header(token, secure));
    response
}

pub fn without_session_cookie(mut response: Response) -> Response {
    response.headers_mut().append(
        header::SET_COOKIE,
        HeaderValue::from_static("session_token=; Expires=Thu, 01 Jan 1970 00:00:00 GMT; Max-Age=0; Path=/"),
    );
    response
}

/// Csak az adminnál kerül a válaszba az `is_admin` jelző; másnál a kulcs sem szerepel (egy `false` érték is elárulná,
/// hogy admin panel létezik).
fn with_admin_flag(app: &App, mut user: Value, email: &str) -> Value {
    if app.is_admin_email(email) {
        user["is_admin"] = json!(true);
    }
    user
}

fn too_many() -> Response {
    fail(429, "Túl sok kérés. Próbáld újra később.")
}

fn unauthorized() -> Response {
    fail(401, "Bejelentkezés szükséges.")
}

fn no_session() -> Response {
    fail(401, "Nincs érvényes session.")
}

fn disabled() -> Response {
    fail(503, "Ez a funkció jelenleg ki van kapcsolva.")
}

fn int_arg(query: &HashMap<String, String>, key: &str) -> Option<Option<i64>> {
    query.get(key).map(|v| v.trim().parse::<i64>().ok())
}

fn tiles_json(word: &str) -> Value {
    match tokenize_word(word) {
        Some(tokens) => json!(tokens.iter().map(|t| t.as_str()).collect::<Vec<_>>()),
        None => Value::Null,
    }
}

// ===================================================================================================
// Oldalak és PWA
// ===================================================================================================

/// A kliens fájlok legutóbbi módosítási ideje (másodperc) — a gyorsítótár verziója.
pub fn asset_version(app: &App) -> i64 {
    ["static/app.js", "static/style.css", "static/i18n-data.js", "static/i18n.js", "templates/index.html"]
        .iter()
        .filter_map(|rel| mtime(&app.base_dir.join(rel)))
        .max()
        .unwrap_or(0)
}

/// Az admin fájlok legutóbbi módosítási ideje.
pub fn admin_asset_version(app: &App) -> i64 {
    std::fs::read_dir(app.base_dir.join("admin_assets"))
        .map(|dir| dir.flatten().filter_map(|e| mtime(&e.path())).max().unwrap_or(0))
        .unwrap_or(0)
}

fn mtime(path: &std::path::Path) -> Option<i64> {
    let modified = std::fs::metadata(path).ok()?.modified().ok()?;
    modified.duration_since(std::time::UNIX_EPOCH).ok().map(|d| d.as_secs() as i64)
}

/// A sablon szövege; a záró újsort (mint a Jinja alapértelmezése) levágja.
fn read_template(app: &App, name: &str) -> Option<String> {
    let text = std::fs::read_to_string(app.base_dir.join("templates").join(name)).ok()?;
    Some(text.strip_suffix('\n').map(|t| t.to_string()).unwrap_or(text))
}

pub fn html_response(body: String) -> Response {
    let mut response = (StatusCode::OK, body).into_response();
    response.headers_mut().insert(header::CONTENT_TYPE, HeaderValue::from_static("text/html; charset=utf-8"));
    response
}

/// Az admin oldal sablonja (a `{{ asset_v }}` és `{{ public_v }}` behelyettesítésével).
pub fn render_admin_page(app: &App) -> Option<String> {
    Some(
        read_template(app, "admin.html")?
            .replace("{{ asset_v }}", &admin_asset_version(app).to_string())
            .replace("{{ public_v }}", &asset_version(app).to_string()),
    )
}

async fn index(State(app): State<Arc<App>>) -> Response {
    match read_template(&app, "index.html") {
        Some(template) => html_response(template.replace("{{ asset_v }}", &asset_version(&app).to_string())),
        None => (StatusCode::INTERNAL_SERVER_ERROR, "Hiányzik a templates/index.html").into_response(),
    }
}

async fn manifest(State(app): State<Arc<App>>) -> Response {
    match std::fs::read(app.base_dir.join("static/manifest.webmanifest")) {
        Ok(bytes) => {
            let mut response = (StatusCode::OK, bytes).into_response();
            response.headers_mut().insert(header::CONTENT_TYPE, HeaderValue::from_static("application/manifest+json"));
            response.headers_mut().insert(header::CACHE_CONTROL, HeaderValue::from_static("no-cache"));
            response
        }
        Err(_) => not_found(),
    }
}

/// A service worker a gyökérről szolgálódik ki, hogy az egész oldalra érvényes legyen.
async fn service_worker(State(app): State<Arc<App>>) -> Response {
    let Some(template) = read_template(&app, "sw.js") else { return not_found() };
    let body = template.replace("{{ version|tojson }}", &asset_version(&app).to_string());
    let mut response = (StatusCode::OK, body).into_response();
    let headers = response.headers_mut();
    headers.insert(header::CONTENT_TYPE, HeaderValue::from_static("application/javascript; charset=utf-8"));
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-cache"));
    headers.insert("service-worker-allowed", HeaderValue::from_static("/"));
    response
}

/// Egész szám útvonal-paraméter (a Flask `<int:...>` konvertere): nem szám → a szokásos 404.
pub struct IdPath(pub i64);

impl<S: Send + Sync> axum::extract::FromRequestParts<S> for IdPath {
    type Rejection = Response;

    async fn from_request_parts(parts: &mut axum::http::request::Parts, state: &S) -> Result<Self, Self::Rejection> {
        let Path(raw): Path<String> = Path::from_request_parts(parts, state).await.map_err(|_| not_found())?;
        if raw.is_empty() || !raw.bytes().all(|b| b.is_ascii_digit()) {
            return Err(not_found());
        }
        raw.parse::<i64>().map(IdPath).map_err(|_| not_found())
    }
}

/// A szokásos 405 (rossz metódus).
pub fn method_not_allowed() -> Response {
    let body = "<!doctype html>\n<html lang=en>\n<title>405 Method Not Allowed</title>\n<h1>Method Not Allowed</h1>\n<p>The method is not allowed for the requested URL.</p>\n";
    let mut response = (StatusCode::METHOD_NOT_ALLOWED, body).into_response();
    response.headers_mut().insert(header::CONTENT_TYPE, HeaderValue::from_static("text/html; charset=utf-8"));
    response
}

/// A szokásos 404 (az admin őr is ezt adja nem adminnak: a két válasz azonos).
pub fn not_found() -> Response {
    let body = "<!doctype html>\n<html lang=en>\n<title>404 Not Found</title>\n<h1>Not Found</h1>\n<p>The requested URL was not found on the server. If you entered the URL manually please check your spelling and try again.</p>\n";
    let mut response = (StatusCode::NOT_FOUND, body).into_response();
    response.headers_mut().insert(header::CONTENT_TYPE, HeaderValue::from_static("text/html; charset=utf-8"));
    response
}

// ===================================================================================================
// Hitelesítés
// ===================================================================================================

async fn request_code(State(app): State<Arc<App>>, client: Client, body: Bytes) -> Response {
    if !app.limiter.check_ip(&client.ip, "request_code") {
        return fail(429, "Túl sok kérés. Próbáld újra 5 perc múlva.");
    }
    if !app.settings.get_bool("registration_open") {
        return fail(403, "A regisztráció jelenleg le van zárva.");
    }
    let data = json_body(&client.headers, &body);
    let Some(email) = data.as_ref().and_then(|d| d.get("email")).and_then(|e| e.as_str()) else {
        return fail(400, "Email cím megadása kötelező.");
    };
    let email = email.trim().to_lowercase();
    if !EMAIL_RE.is_match(&email) || email.chars().count() > 254 {
        return fail(400, "Érvénytelen email cím.");
    }
    if app.db.get_user_by_email(&email).ok().flatten().is_some() {
        return fail(409, "Ez az email cím már regisztrálva van.");
    }
    let Ok(code) = app.db.create_verification_code(&email) else { return fail(500, "Belső hiba.") };
    app.mailer.send_verification_email(&email, &code);

    let mut response = json!({"success": true, "message": "Verifikációs kód elküldve."});
    if !app.mailer.is_configured() {
        response["message"] = json!("Fejlesztői mód: SMTP nincs konfigurálva.");
        // Admin címnél a kód nem kerülhet a válaszba (a szerver konzolján olvasható): SMTP nélkül különben bárki
        // „megerősíthetné” az admin címet, és a regisztrációval admin lenne.
        if !app.is_admin_email(&email) {
            response["dev_code"] = json!(code);
        }
    }
    ok(response)
}

async fn verify_code(State(app): State<Arc<App>>, client: Client, body: Bytes) -> Response {
    let Some(data) = json_body(&client.headers, &body).filter(|d| d.as_object().is_some_and(|o| !o.is_empty())) else {
        return fail(400, "Érvénytelen kérés.");
    };
    let email = str_field(&data, "email").to_lowercase();
    let code = str_field(&data, "code");
    if email.is_empty() || code.is_empty() {
        return fail(400, "Email és kód megadása kötelező.");
    }
    if code.len() != 6 || !code.bytes().all(|b| b.is_ascii_digit()) {
        return fail(400, "A kód 6 számjegyből áll.");
    }
    let (success, message) = app.db.verify_code(&email, &code);
    json_response(if success { 200 } else { 400 }, json!({"success": success, "message": message}))
}

async fn register(State(app): State<Arc<App>>, client: Client, body: Bytes) -> Response {
    if !app.limiter.check_ip(&client.ip, "register") {
        return fail(429, "Túl sok regisztráció. Próbáld újra később.");
    }
    if !app.settings.get_bool("registration_open") {
        return fail(403, "A regisztráció jelenleg le van zárva.");
    }
    let Some(data) = json_body(&client.headers, &body).filter(|d| d.as_object().is_some_and(|o| !o.is_empty())) else {
        return fail(400, "Érvénytelen kérés.");
    };
    let email = str_field(&data, "email").to_lowercase();
    let password = data.get("password").and_then(|p| p.as_str()).unwrap_or("").to_string();
    let display_name = str_field(&data, "display_name");
    if email.is_empty() || password.is_empty() || display_name.is_empty() {
        return fail(400, "Minden mező kitöltése kötelező.");
    }
    if !EMAIL_RE.is_match(&email) || email.chars().count() > 254 {
        return fail(400, "Érvénytelen email cím.");
    }
    let password_len = password.chars().count();
    if password_len < 6 {
        return fail(400, "A jelszó legalább 6 karakter legyen.");
    }
    if password_len > 128 {
        return fail(400, "A jelszó maximum 128 karakter lehet.");
    }
    let Some(name) = sanitize_name_str(&display_name) else {
        return fail(400, "Érvénytelen megjelenítési név (1-20 karakter, betűk és számok).");
    };
    if moderation::contains_banned(&app.banned_word_list(), &name) {
        return fail(400, "Ez a név nem engedélyezett.");
    }
    if app.db.get_user_by_email(&email).ok().flatten().is_some() {
        return fail(409, "Ez az email cím már regisztrálva van.");
    }
    if !app.db.is_email_verified(&email) {
        return fail(403, "Az email címet előbb meg kell erősíteni a kóddal.");
    }
    let user_id = match app.db.create_user(&email, &name, &password) {
        Ok(id) => id,
        Err(message) => return fail(409, &message),
    };
    app.db.clear_email_verification(&email);
    let Ok(token) = app.db.create_session(user_id, Some(&client.ip), Some(&client.user_agent)) else { return fail(500, "Belső hiba.") };
    app.db.record_login_event(&email, true, Some(&client.ip), Some(&client.user_agent), Some(user_id), "register");
    if app.is_admin_email(&email) {
        app.db.touch_admin_session(&token); // a jelszó most hangzott el: az admin panel azonnal nyitható
    }
    let user = with_admin_flag(&app, json!({"id": user_id, "email": email, "display_name": name}), &email);
    with_session_cookie(ok(json!({"success": true, "message": "Fiók létrehozva!", "user": user})), &token, client.secure)
}

async fn login(State(app): State<Arc<App>>, client: Client, body: Bytes) -> Response {
    if !app.limiter.check_ip(&client.ip, "login") {
        return fail(429, "Túl sok bejelentkezési kísérlet. Próbáld újra 5 perc múlva.");
    }
    let Some(data) = json_body(&client.headers, &body).filter(|d| d.as_object().is_some_and(|o| !o.is_empty())) else {
        return fail(400, "Érvénytelen kérés.");
    };
    let email = str_field(&data, "email").to_lowercase();
    let password = data.get("password").and_then(|p| p.as_str()).unwrap_or("").to_string();
    if email.is_empty() || password.is_empty() {
        return fail(400, "Email és jelszó megadása kötelező.");
    }
    let user = match app.db.verify_password(&email, &password) {
        Ok(user) => user,
        Err(message) => {
            app.db.record_login_event(&email, false, Some(&client.ip), Some(&client.user_agent), None, "bad_credentials");
            return fail(401, &message);
        }
    };
    if let Some(ban) = user.ban() {
        app.db.record_login_event(&email, false, Some(&client.ip), Some(&client.user_agent), Some(user.id), "banned");
        return json_response(403, json!({"success": false, "message": "A fiókod ki van tiltva.", "banned": ban.to_json()}));
    }
    let Ok(token) = app.db.create_session(user.id, Some(&client.ip), Some(&client.user_agent)) else { return fail(500, "Belső hiba.") };
    app.db.record_login_event(&email, true, Some(&client.ip), Some(&client.user_agent), Some(user.id), "");
    if app.is_admin_email(&user.email) {
        app.db.touch_admin_session(&token); // a jelszó most hangzott el: az admin panel azonnal nyitható
    }
    let payload = with_admin_flag(&app, json!({"id": user.id, "email": user.email, "display_name": user.display_name}), &user.email);
    with_session_cookie(ok(json!({"success": true, "message": "Sikeres bejelentkezés!", "user": payload})), &token, client.secure)
}

async fn logout(State(app): State<Arc<App>>, client: Client) -> Response {
    if let Some(token) = &client.token {
        app.db.delete_session(Some(token));
    }
    without_session_cookie(ok(json!({"success": true, "message": "Kijelentkezve."})))
}

async fn me(State(app): State<Arc<App>>, client: Client) -> Response {
    let Some(user) = client.user(&app) else { return no_session() };
    let payload = with_admin_flag(
        &app,
        json!({
            "id": user.id, "email": user.email, "display_name": user.display_name,
            "games_played": user.games_played, "games_won": user.games_won, "total_score": user.total_score,
        }),
        &user.email,
    );
    ok(json!({"success": true, "user": payload}))
}

/// Rövid életű token a Socket.IO `set_name` identitás igazolásához.
async fn socket_token(State(app): State<Arc<App>>, client: Client) -> Response {
    let Some(user) = client.user(&app) else { return no_session() };
    ok(json!({"success": true, "token": create_socket_token(&app.config.secret_key, user.id)}))
}

fn round1(value: f64) -> f64 {
    (value * 10.0).round() / 10.0
}

async fn profile(State(app): State<Arc<App>>, client: Client) -> Response {
    let Some(user) = client.user(&app) else { return no_session() };
    let (played, won, total) = (user.games_played, user.games_won, user.total_score);
    let win_rate = if played > 0 { round1(won as f64 / played as f64 * 100.0) } else { 0.0 };
    let avg_score = if played > 0 { round1(total as f64 / played as f64) } else { 0.0 };
    let history = app.db.get_user_game_history(user.id, 20).unwrap_or_default();
    let history: Vec<Value> = history
        .iter()
        .map(|h| {
            let change = match (h["rating_before"].as_i64(), h["rating_after"].as_i64()) {
                (Some(before), Some(after)) => json!(after - before),
                _ => Value::Null,
            };
            json!({
                "game_id": h["game_id"], "room_name": h["room_name"], "created_at": h["created_at"],
                "final_score": h["final_score"], "is_winner": util::truthy(&h["is_winner"]),
                "rating_change": change, "opponents": h["opponents"],
            })
        })
        .collect();
    ok(json!({
        "success": true,
        "badges": app.db.get_user_achievements(user.id),
        "stats": {
            "games_played": played, "games_won": won, "win_rate": win_rate, "avg_score": avg_score,
            "total_score": total, "rating": user.rating, "rated_games": user.rated_games,
        },
        "history": history,
    }))
}

fn is_saved_game_owner(game: &db::Row, user: &db::SessionUser) -> bool {
    let owner_token = game.opt_text("owner_token").filter(|t| !t.is_empty());
    match (owner_token, user.reconnect_token.as_deref().filter(|t| !t.is_empty())) {
        (Some(game_token), Some(user_token)) => game_token == user_token,
        _ => game.opt_text("owner_name").unwrap_or_default() == user.display_name,
    }
}

async fn saved_games(State(app): State<Arc<App>>, client: Client) -> Response {
    if client.token.is_none() {
        return fail(401, "Nincs session.");
    }
    let Some(user) = client.user(&app) else { return no_session() };
    let games = app.db.get_user_active_games(user.id, user.reconnect_token.as_deref()).unwrap_or_default();
    let list: Vec<Value> = games
        .iter()
        .map(|g| {
            json!({
                "game_id": g.int("game_id"), "room_name": g.text("room_name"), "room_id": g.text("room_id"),
                "created_at": g.text("created_at"), "updated_at": g.text("updated_at"),
                "challenge_mode": g.flag("challenge_mode"), "player_name": g.text("player_name"),
                "score": g.int("final_score"), "opponents": g.get("opponents").cloned().unwrap_or(json!([])),
                "owner_name": g.opt_text("owner_name").unwrap_or_default(),
                "is_owner": is_saved_game_owner(g, &user),
            })
        })
        .collect();
    ok(json!({"success": true, "games": list}))
}

async fn friends(State(app): State<Arc<App>>, client: Client) -> Response {
    let Some(user) = client.user(&app) else { return no_session() };
    let online = app.state.lock().get_online_user_ids();
    let list: Vec<Value> = app
        .db
        .get_friends(user.id)
        .into_iter()
        .map(|mut f| {
            let is_online = online.contains(&f.int("id"));
            f.insert("online".into(), json!(is_online));
            Value::Object(f)
        })
        .collect();
    ok(json!({
        "success": true,
        "friends": list,
        "pending_requests": app.db.get_pending_requests(user.id),
        "sent_requests": app.db.get_sent_requests(user.id),
    }))
}

async fn search_users(State(app): State<Arc<App>>, client: Client, Query(query): Query<HashMap<String, String>>) -> Response {
    if !app.limiter.check_ip(&client.ip, "search_users") {
        return fail(429, "Túl sok keresés. Próbáld újra később.");
    }
    let Some(user) = client.user(&app) else { return no_session() };
    let q = query.get("q").map(|q| q.trim().to_string()).unwrap_or_default();
    if q.chars().count() < 2 {
        return ok(json!({"success": true, "users": []}));
    }
    ok(json!({"success": true, "users": app.db.search_users(&q, user.id, 10)}))
}

// ===================================================================================================
// Nyilvános API: ranglista, napi feladvány, közlemények
// ===================================================================================================

async fn leaderboard(State(app): State<Arc<App>>, client: Client, Query(query): Query<HashMap<String, String>>) -> Response {
    if !app.limiter.check_ip(&client.ip, "leaderboard") {
        return too_many();
    }
    let metric = query.get("metric").map(|m| m.as_str()).unwrap_or("wins");
    if !LEADERBOARD_METRICS.contains(&metric) {
        return fail(400, "Ismeretlen rangsor.");
    }
    let limit = int_arg(&query, "limit").map(|l| l.unwrap_or(50)).unwrap_or(50);
    let my_id = client.user(&app).map(|u| u.id);
    let (mut entries, me) = app.db.get_leaderboard(metric, limit, my_id, false).unwrap_or_default();
    for entry in &mut entries {
        let is_me = my_id.is_some() && entry["user_id"].as_i64() == my_id;
        entry["is_me"] = json!(is_me);
    }
    ok(json!({
        "success": true, "metric": metric, "min_games": db::leaderboard_min_games(metric),
        "entries": entries, "me": me,
    }))
}

fn mark_me(entries: &mut [Value], user_id: Option<i64>) {
    for entry in entries {
        let is_me = user_id.is_some() && entry["user_id"].as_i64() == user_id;
        entry["is_me"] = json!(is_me);
    }
}

/// A mai napi feladvány adatai: a saját eredményed, a nap ranglistájának eleje és a tegnapi megoldás. A feladvány
/// legjobb pontszáma csak azoknak látszik, akik már beküldtek egy lépést.
async fn daily_info(State(app): State<Arc<App>>, client: Client) -> Response {
    if !app.limiter.check_ip(&client.ip, "daily") {
        return too_many();
    }
    if !app.settings.get_bool("feature_daily") {
        return disabled();
    }
    let user_id = client.user(&app).map(|u| u.id);
    let today = daily::today_str();
    let (mut entries, me) = app.db.get_daily_leaderboard(&today, 10, user_id);
    let mine = user_id.and_then(|uid| app.db.get_daily_entry(&today, uid));
    let played = mine.as_ref().is_some_and(|m| m.attempts > 0);
    let revealed = mine.as_ref().is_some_and(|m| m.revealed);
    let puzzle = app.db.get_daily_puzzle(&today);
    let yesterday = app.db.get_daily_puzzle(&daily::previous_date(&today)).map(|y| {
        let (top, _) = app.db.get_daily_leaderboard(&y.date, 3, None);
        json!({"date": y.date, "best_score": y.best_score, "best_words": y.best["words"], "top": top})
    });
    mark_me(&mut entries, user_id);
    ok(json!({
        "success": true, "date": today, "ready": puzzle.is_some(),
        "my": mine.map(|m| m.to_json()),
        "best_score": puzzle.as_ref().filter(|_| played || revealed).map(|p| p.best_score),
        "leaderboard": entries, "me": me, "yesterday": yesterday,
    }))
}

/// Egy nap ranglistája (alapértelmezés: ma; csak a mai és a korábbi napok kérhetők).
async fn daily_leaderboard(State(app): State<Arc<App>>, client: Client, Query(query): Query<HashMap<String, String>>) -> Response {
    if !app.limiter.check_ip(&client.ip, "daily") {
        return too_many();
    }
    let today = daily::today_str();
    let date = query.get("date").cloned().unwrap_or_else(|| today.clone());
    if !daily::is_valid_date(&date) || date > today {
        return fail(400, "Érvénytelen dátum.");
    }
    let user_id = client.user(&app).map(|u| u.id);
    let (mut entries, me) = app.db.get_daily_leaderboard(&date, 50, user_id);
    mark_me(&mut entries, user_id);
    ok(json!({"success": true, "date": date, "entries": entries, "me": me}))
}

/// A most érvényes közlemények (banner) és a karbantartási mód — nyilvános; a célcsoport a bejelentkezéstől függ.
async fn announcements(State(app): State<Arc<App>>, client: Client) -> Response {
    if !app.limiter.check_ip(&client.ip, "announcements") {
        return too_many();
    }
    let registered = client.user(&app).is_some();
    ok(json!({"success": true, "items": comm::active_announcements(&app, registered)}))
}

// ===================================================================================================
// Web Push
// ===================================================================================================

async fn push_public_key(State(app): State<Arc<App>>, client: Client) -> Response {
    let Some(user) = client.user(&app) else { return unauthorized() };
    if !app.push.is_available() {
        return ok(json!({"success": true, "available": false}));
    }
    ok(json!({
        "success": true, "available": true, "public_key": app.push.public_key(),
        "subscribed": app.db.count_push_subscriptions(user.id) > 0,
    }))
}

async fn push_subscribe(State(app): State<Arc<App>>, client: Client, body: Bytes) -> Response {
    let Some(user) = client.user(&app) else { return unauthorized() };
    if !app.limiter.check_ip(&client.ip, "push") {
        return too_many();
    }
    if !app.push.is_available() {
        return fail(503, "Az értesítések ezen a szerveren nem érhetők el.");
    }
    let data = json_object(&client.headers, &body);
    let keys = data.get("keys").filter(|k| k.is_object()).cloned().unwrap_or(json!({}));
    let endpoint = data.get("endpoint").and_then(|e| e.as_str());
    let p256dh = keys.get("p256dh").and_then(|k| k.as_str());
    let auth_key = keys.get("auth").and_then(|k| k.as_str());
    fn valid(text: Option<&str>, low: usize, high: usize) -> Option<&str> {
        text.filter(|t| (low..=high).contains(&t.chars().count()))
    }
    let (Some(endpoint), Some(p256dh), Some(auth_key)) = (
        endpoint.filter(|e| PUSH_ENDPOINT_RE.is_match(e)),
        valid(p256dh, 20, 200),
        valid(auth_key, 8, 100),
    ) else {
        return fail(400, "Érvénytelen feliratkozás.");
    };
    let lang = data.get("lang").and_then(|l| l.as_str()).filter(|l| push::is_supported_lang(l)).unwrap_or(push::DEFAULT_LANG);
    let _ = app.db.save_push_subscription(user.id, endpoint, p256dh, auth_key, lang);
    ok(json!({"success": true}))
}

async fn push_unsubscribe(State(app): State<Arc<App>>, client: Client, body: Bytes) -> Response {
    let Some(user) = client.user(&app) else { return unauthorized() };
    let data = json_object(&client.headers, &body);
    let Some(endpoint) = data.get("endpoint").and_then(|e| e.as_str()) else {
        return fail(400, "Érvénytelen feliratkozás.");
    };
    app.db.delete_push_subscription(endpoint, Some(user.id));
    ok(json!({"success": true}))
}

// ===================================================================================================
// Gyakorló módok
// ===================================================================================================

/// Közös ellenőrzés a gyakorló végpontokhoz: kapcsoló, forgalomkorlát + szótár. Hibaválasz, vagy None. `counter`: a
/// használati statisztika kulcsa (naponta számolva).
fn practice_guard(app: &App, client: &Client, counter: Option<&str>) -> Option<Response> {
    if !app.settings.get_bool("feature_practice") {
        return Some(disabled());
    }
    if !app.limiter.check_ip(&client.ip, "practice") {
        return Some(too_many());
    }
    if !dictionary::is_available() {
        return Some(fail(503, "A szótár nem érhető el."));
    }
    if let Some(counter) = counter {
        app.db.bump_counter(counter, 1);
    }
    None
}

async fn practice_quiz(State(app): State<Arc<App>>, client: Client, Query(query): Query<HashMap<String, String>>) -> Response {
    let mode = query.get("mode").cloned().unwrap_or_else(|| "mixed".to_string());
    if !practice::QUIZ_MODES.contains(&mode.as_str()) {
        return fail(400, "Érvénytelen kvízmód.");
    }
    if let Some(blocked) = practice_guard(&app, &client, Some("quiz")) {
        return blocked;
    }
    let count = int_arg(&query, "n").map(|n| n.unwrap_or(practice::QUESTION_COUNT as i64)).unwrap_or(practice::QUESTION_COUNT as i64);
    let questions = tokio::task::spawn_blocking(move || practice::make_quiz(count.max(0) as usize, &mode, &mut rand::rng()).map(|q| (mode, q))).await;
    match questions {
        Ok(Ok((mode, questions))) => {
            let tiles: Vec<Value> = questions.iter().map(|q| tiles_json(q)).collect();
            ok(json!({"success": true, "mode": mode, "questions": questions, "tiles": tiles}))
        }
        _ => fail(503, "A szótár nem érhető el."),
    }
}

async fn practice_answer(State(app): State<Arc<App>>, client: Client, body: Bytes) -> Response {
    let data = json_object(&client.headers, &body);
    let (Some(word), Some(answer)) = (data.get("word").and_then(|w| w.as_str()), data.get("answer").and_then(|a| a.as_bool())) else {
        return fail(400, "Érvénytelen kérés.");
    };
    if let Some(blocked) = practice_guard(&app, &client, Some("answer")) {
        return blocked;
    }
    match practice::check_answer(word, answer) {
        Some(result) => success_with(result),
        None => fail(400, "Érvénytelen szó."),
    }
}

async fn practice_short_words(State(app): State<Arc<App>>, client: Client, Query(query): Query<HashMap<String, String>>) -> Response {
    let length = int_arg(&query, "length").map(|l| l.unwrap_or(0)).unwrap_or(2);
    if !practice::SHORT_LENGTHS.iter().any(|l| *l as i64 == length) {
        return fail(400, "Érvénytelen szóhossz.");
    }
    if let Some(blocked) = practice_guard(&app, &client, Some("short_words")) {
        return blocked;
    }
    let words = tokio::task::spawn_blocking(move || practice::short_words(length as usize)).await.unwrap_or_default();
    ok(json!({"success": true, "length": length, "words": words}))
}

async fn practice_rack(State(app): State<Arc<App>>, client: Client, Query(query): Query<HashMap<String, String>>) -> Response {
    let kind = query.get("kind").cloned().unwrap_or_else(|| "hunt".to_string());
    if !practice::RACK_KINDS.contains(&kind.as_str()) {
        return fail(400, "Érvénytelen gyakorlás.");
    }
    if let Some(blocked) = practice_guard(&app, &client, Some("rack")) {
        return blocked;
    }
    match tokio::task::spawn_blocking(move || practice::make_rack(&kind, &mut rand::rng())).await {
        Ok(Ok(rack)) => success_with(rack),
        _ => fail(503, "A szótár nem érhető el."),
    }
}

async fn practice_rack_word(State(app): State<Arc<App>>, client: Client, body: Bytes) -> Response {
    let data = json_object(&client.headers, &body);
    let rack: Option<Vec<String>> = data.get("rack").and_then(|r| r.as_array()).and_then(|list| {
        if !(1..=practice::RACK_SIZE).contains(&list.len()) {
            return None;
        }
        list.iter().map(|t| t.as_str().filter(|t| tiles::Tile::from_str(t).is_some_and(|tile| !tile.is_blank())).map(|t| t.to_string())).collect()
    });
    let (Some(rack), Some(word)) = (rack, data.get("word").and_then(|w| w.as_str())) else {
        return fail(400, "Érvénytelen kérés.");
    };
    if let Some(blocked) = practice_guard(&app, &client, Some("rack_word")) {
        return blocked;
    }
    success_with(practice::check_rack_word(&rack, word))
}

// --- Szótár-építő ---

/// A szótár-építő végpontjainak közös őre: bejelentkezés, forgalomkorlát, szótár.
fn word_review_user(app: &App, client: &Client) -> Result<db::SessionUser, Response> {
    let Some(user) = client.user(app) else { return Err(unauthorized()) };
    if !app.settings.get_bool("feature_word_review") {
        return Err(disabled());
    }
    if app.db.is_review_blocked(user.id) {
        return Err(fail(403, "A szótár-építő használata le van tiltva a fiókodnál."));
    }
    if !app.limiter.check_ip(&client.ip, "word_review") {
        return Err(too_many());
    }
    if !dictionary::is_available() {
        return Err(fail(503, "A szótár nem érhető el."));
    }
    Ok(user)
}

async fn word_review_next(State(app): State<Arc<App>>, client: Client, Query(query): Query<HashMap<String, String>>) -> Response {
    let user = match word_review_user(&app, &client) {
        Ok(user) => user,
        Err(response) => return response,
    };
    let count = int_arg(&query, "n").map(|n| n.unwrap_or(word_review::BATCH_SIZE as i64)).unwrap_or(word_review::BATCH_SIZE as i64);
    let db = app.db.clone();
    let words = tokio::task::spawn_blocking(move || word_review::next_words(&db, count.max(1) as usize, &mut rand::rng())).await.unwrap_or_default();
    app.db.bump_counter("word_review", 1);
    let tiles: Vec<Value> = words.iter().map(|w| tiles_json(w)).collect();
    ok(json!({"success": true, "words": words, "tiles": tiles, "stats": word_review::stats(&app.db, user.id)}))
}

async fn word_review_vote(State(app): State<Arc<App>>, client: Client, body: Bytes) -> Response {
    let user = match word_review_user(&app, &client) {
        Ok(user) => user,
        Err(response) => return response,
    };
    let data = json_object(&client.headers, &body);
    let word = data.get("word").and_then(word_review::normalize);
    let (Some(word), Some(valid)) = (word, data.get("valid").and_then(|v| v.as_bool())) else {
        return fail(400, "Érvénytelen kérés.");
    };
    let rejected = word_review::record_vote(&app.db, &app.settings, user.id, &word, valid);
    app.db.bump_counter("word_review_vote", 1);
    match rejected {
        None => fail(409, "Ez a szó már nincs a szótárban."),
        Some(rejected) => ok(json!({"success": true, "word": word, "rejected": rejected, "stats": word_review::stats(&app.db, user.id)})),
    }
}

async fn word_review_undo(State(app): State<Arc<App>>, client: Client, body: Bytes) -> Response {
    let user = match word_review_user(&app, &client) {
        Ok(user) => user,
        Err(response) => return response,
    };
    let data = json_object(&client.headers, &body);
    let Some(word) = data.get("word").and_then(word_review::normalize) else {
        return fail(400, "Érvénytelen kérés.");
    };
    let (done, rejected) = word_review::undo_vote(&app.db, &app.settings, user.id, &word);
    if !done {
        return fail(404, "Nincs mit visszavonni.");
    }
    ok(json!({"success": true, "word": word, "rejected": rejected, "stats": word_review::stats(&app.db, user.id)}))
}

async fn word_review_stats(State(app): State<Arc<App>>, client: Client) -> Response {
    match word_review_user(&app, &client) {
        Ok(user) => ok(json!({"success": true, "stats": word_review::stats(&app.db, user.id)})),
        Err(response) => response,
    }
}

// ===================================================================================================
// Szótár-ellenőrző
// ===================================================================================================

async fn dictionary_check(State(app): State<Arc<App>>, client: Client, method: axum::http::Method, Query(query): Query<HashMap<String, String>>, body: Bytes) -> Response {
    if !app.limiter.check_ip(&client.ip, "dictionary") {
        return too_many();
    }
    let raw: String = if method == axum::http::Method::POST {
        match json_body(&client.headers, &body) {
            Some(Value::Object(data)) => match data.get("words").or_else(|| data.get("q")) {
                Some(Value::Array(list)) => list
                    .iter()
                    .filter_map(|w| match w {
                        Value::String(s) => Some(s.clone()),
                        Value::Number(n) => Some(n.to_string()),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
                    .join(" "),
                Some(Value::String(s)) => s.clone(),
                _ => String::new(),
            },
            _ => String::new(),
        }
    } else {
        query.get("q").cloned().unwrap_or_default()
    };
    // a szavak száma és hossza úgyis korlátozott: ne dolgozzunk fel óriás bemenetet
    let raw: String = raw.chars().take(300).collect();

    let mut words: Vec<String> = Vec::new();
    for token in raw.split(|c: char| c.is_whitespace() || c == ',' || c == ';') {
        let token = token.trim();
        if !token.is_empty() && !words.iter().any(|w| w.to_uppercase() == token.to_uppercase()) {
            words.push(token.to_string());
        }
    }
    if words.is_empty() {
        return fail(400, "Adj meg legalább egy szót.");
    }
    words.truncate(MAX_DICT_WORDS);
    if !dictionary::is_available() {
        // Szótár híján a játék minden szót elfogadna: a Szótár-eszköz ne mondjon hamis „érvényes”-t
        return fail(503, "A szótár jelenleg nem elérhető.");
    }

    let mut results: Vec<Value> = Vec::new();
    let mut checkable: Vec<usize> = Vec::new();
    for word in &words {
        let upper = word.to_uppercase();
        let length = upper.chars().count();
        let tokens = if length <= 15 { tokenize_word(&upper) } else { None };
        let mut entry = json!({
            "word": upper.chars().take(30).collect::<String>(), "valid": false, "tiles": [], "score": null,
            "reason": null, "suggestions": [],
        });
        if length > 15 {
            entry["reason"] = json!("too_long");
        } else if let Some(tokens) = tokens {
            let letters: Vec<&str> = tokens.iter().map(|t| t.as_str()).collect();
            entry["tiles"] = json!(letters);
            entry["score"] = json!(tokens.iter().map(|t| tiles::tile_value(t.as_str())).sum::<u32>());
            if tokens.len() < 2 {
                entry["reason"] = json!("too_short");
            } else {
                checkable.push(results.len());
            }
        } else {
            entry["reason"] = json!("invalid_chars");
        }
        results.push(entry);
    }

    let check_words: Vec<String> = checkable.iter().map(|i| results[*i]["word"].as_str().unwrap_or("").to_string()).collect();
    let valid = tokio::task::spawn_blocking({
        let check_words = check_words.clone();
        move || dictionary::filter_valid(check_words.iter().map(|w| w.as_str()))
    })
    .await
    .unwrap_or_default();
    let mut suggested = 0;
    for index in checkable {
        let word = results[index]["word"].as_str().unwrap_or("").to_string();
        let is_valid = valid.contains(&word);
        results[index]["valid"] = json!(is_valid);
        if !is_valid {
            results[index]["reason"] = json!("not_in_dictionary");
            if suggested < MAX_SUGGESTION_WORDS {
                results[index]["suggestions"] = json!(dictionary::suggest_words(&word, 5));
                suggested += 1;
            }
        }
    }
    ok(json!({"success": true, "results": results}))
}

// ===================================================================================================
// Játék API
// ===================================================================================================

/// A lépés részletei a játékos keze (`rack`) nélkül.
fn without_rack(details_json: Option<&str>) -> Value {
    let Some(text) = details_json else { return Value::Null };
    match serde_json::from_str::<Value>(text) {
        Ok(Value::Object(mut details)) if details.contains_key("rack") => {
            details.remove("rack");
            json!(Value::Object(details).to_string())
        }
        _ => json!(text),
    }
}

/// A lépésnapló a kliensnek. `hide_racks`: folyamatban lévő játéknál a kezek rejtve maradnak (különben a játékosok
/// lépésenként látnák egymás zsetonjait).
fn moves_payload(app: &App, game_id: i64, hide_racks: bool) -> Vec<Value> {
    app.db
        .get_game_moves(game_id)
        .unwrap_or_default()
        .iter()
        .map(|m| {
            json!({
                "move_number": m.move_number, "player_name": m.player_name, "action_type": m.action_type,
                "details_json": if hide_racks { without_rack(m.details_json.as_deref()) } else { json!(m.details_json) },
                "board_snapshot_json": m.board_snapshot_json,
            })
        })
        .collect()
}

/// Megosztható linket készít egy befejezett játék visszajátszásához (csak a résztvevőknek).
async fn share_game(State(app): State<Arc<App>>, client: Client, IdPath(game_id): IdPath) -> Response {
    let Some(user) = client.user(&app) else { return unauthorized() };
    if app.db.get_game_by_id(game_id).ok().flatten().is_none() {
        return fail(404, "Játék nem található.");
    }
    if !app.db.is_user_in_game(game_id, user.id) {
        return fail(403, "Nincs jogosultságod a játék megosztásához.");
    }
    match app.db.get_or_create_share_token(game_id).ok().flatten() {
        Some(token) => ok(json!({"success": true, "token": token})),
        None => fail(400, "Csak befejezett játék osztható meg."),
    }
}

/// Megosztott visszajátszás: nyilvános, bejelentkezés nélkül is elérhető.
async fn shared_replay(State(app): State<Arc<App>>, client: Client, Path(token): Path<String>) -> Response {
    if !app.limiter.check_ip(&client.ip, "replay") {
        return too_many();
    }
    let row = if SHARE_TOKEN_RE.is_match(&token) { app.db.get_game_by_share_token(&token).ok().flatten() } else { None };
    let Some(row) = row else { return fail(404, "A megosztott visszajátszás nem található.") };
    let game_id = row.int("id");
    ok(json!({
        "success": true, "room_name": row.text("room_name"), "created_at": row.text("created_at"),
        "players": app.db.get_game_results(game_id).unwrap_or_default(),
        "moves": moves_payload(&app, game_id, false),
    }))
}

async fn game_moves(State(app): State<Arc<App>>, client: Client, IdPath(game_id): IdPath) -> Response {
    let Some(user) = client.user(&app) else { return unauthorized() };
    let Some(row) = app.db.get_game_by_id(game_id).ok().flatten() else { return fail(404, "Játék nem található.") };
    if !app.db.is_user_in_game(game_id, user.id) {
        return fail(403, "Nincs jogosultságod a játék megtekintéséhez.");
    }
    let finished = row.text("status") == "finished";
    ok(json!({
        "success": true, "moves": moves_payload(&app, game_id, !finished),
        "players": app.db.get_game_results(game_id).unwrap_or_default(), "finished": finished,
    }))
}

/// A befejezett játék elemzése: lépésenként a legjobb lehetséges lépés és a kint maradt pont. A számítás háttérben
/// fut; amíg tart, `status: 'running'` (a kliens időnként újrakérdezi).
async fn game_analysis(State(app): State<Arc<App>>, client: Client, IdPath(game_id): IdPath) -> Response {
    let Some(user) = client.user(&app) else { return unauthorized() };
    if !app.limiter.check_ip(&client.ip, "analysis") {
        return too_many();
    }
    let Some(row) = app.db.get_game_by_id(game_id).ok().flatten() else { return fail(404, "Játék nem található.") };
    if !app.db.is_user_in_game(game_id, user.id) {
        return fail(403, "Nincs jogosultságod a játék megtekintéséhez.");
    }
    if row.text("status") != "finished" {
        return fail(400, "Csak befejezett játék elemezhető.");
    }
    if let Some((version, result_json)) = app.db.get_game_analysis(game_id) {
        if version == analysis::ANALYSIS_VERSION {
            if let Ok(Value::Object(result)) = serde_json::from_str::<Value>(&result_json) {
                let mut payload = json!({"success": true, "status": "ready"});
                for (k, v) in result {
                    payload[k] = v;
                }
                return ok(payload);
            }
        }
    }
    {
        let jobs = app.analysis_jobs.lock();
        match jobs.get(&game_id) {
            Some(AnalysisJob::Running { done, total }) => {
                return ok(json!({"success": true, "status": "running", "done": done, "total": total}));
            }
            Some(AnalysisJob::Failed { at }) if util::now() - at < ANALYSIS_RETRY_AFTER => {
                return ok(json!({"success": true, "status": "error"}));
            }
            _ => {}
        }
    }
    let moves = app.db.get_game_moves(game_id).unwrap_or_default();
    let total = analysis::analyzable_turns(&moves).len();
    if total == 0 {
        // Régi játék: a lépésnapló nem őrizte meg a kezeket
        return ok(json!({"success": true, "status": "unavailable"}));
    }
    app.analysis_jobs.lock().insert(game_id, AnalysisJob::Running { done: 0, total });
    let app2 = app.clone();
    tokio::task::spawn_blocking(move || run_analysis(&app2, game_id, moves));
    ok(json!({"success": true, "status": "running", "done": 0, "total": total}))
}

/// Háttérfeladat: elemzi a játékot, és elmenti az eredményt.
pub fn run_analysis(app: &Arc<App>, game_id: i64, moves: Vec<db::StoredMove>) {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let vocab = ai::get_vocabulary();
        let mut progress = |done: usize, total: usize| {
            app.analysis_jobs.lock().insert(game_id, AnalysisJob::Running { done, total });
        };
        analysis::analyze_game(&moves, &vocab, analysis::SECONDS_PER_MOVE, Some(&mut progress))
    }));
    match result {
        Ok(result) => {
            if let Err(e) = app.db.save_game_analysis(game_id, analysis::ANALYSIS_VERSION, &result.to_string()) {
                println!("[analysis] Hiba az elemzés mentésénél (game #{game_id}): {e}");
                app.analysis_jobs.lock().insert(game_id, AnalysisJob::Failed { at: util::now() });
            } else {
                app.analysis_jobs.lock().remove(&game_id);
            }
        }
        Err(_) => {
            println!("[analysis] Hiba az elemzésnél (game #{game_id})");
            app.analysis_jobs.lock().insert(game_id, AnalysisJob::Failed { at: util::now() });
        }
    }
}

/// A bejelentkezett felhasználó folyamatban lévő levelezős játékai (akinél a sor, az elöl).
async fn async_games_list(State(app): State<Arc<App>>, client: Client) -> Response {
    let Some(user) = client.user(&app) else { return unauthorized() };
    if !app.settings.get_bool("feature_async") {
        return disabled();
    }
    let mut games: Vec<Value> = app.db.get_user_async_games(user.id).unwrap_or_default().iter().filter_map(crate::async_games::summarize).collect();
    games.sort_by_key(|g| !g["my_turn"].as_bool().unwrap_or(false)); // a rendezés stabil
    let my_turn_count = games.iter().filter(|g| g["my_turn"].as_bool().unwrap_or(false)).count();
    ok(json!({"success": true, "games": games, "my_turn_count": my_turn_count}))
}

async fn abandon_game(State(app): State<Arc<App>>, client: Client, IdPath(game_id): IdPath) -> Response {
    if client.token.is_none() {
        return fail(401, "Nincs session.");
    }
    let Some(user) = client.user(&app) else { return no_session() };
    let Some(row) = app.db.get_game_by_id(game_id).ok().flatten() else { return fail(404, "Játék nem található.") };
    if row.text("status") != "active" {
        return fail(400, "A játék nem aktív.");
    }
    if row.flag("is_async") {
        return fail(400, "A levelezős játékot a játékban, a Feladom gombbal fejezheted be.");
    }
    if !is_saved_game_owner(&row, &user) {
        return fail(403, "Csak a mentés tulajdonosa törölheti a játékot.");
    }
    // A memóriában lévő szoba takarítása, ha létezik
    let room_id = row.text("room_id");
    {
        let mut st = app.state.lock();
        if st.rooms.contains_key(&room_id) {
            st.cleanup_room(&room_id);
            emit_rooms_list(&app, &st);
        }
    }
    app.db.abandon_game_by_id(game_id);
    ok(json!({"success": true}))
}

// ===================================================================================================
// Útvonaltábla
// ===================================================================================================

/// A nyilvános útvonalak (az admin útvonalak külön, az őr mögött).
pub fn public_routes() -> Router<Arc<App>> {
    Router::new()
        .route("/", get(index))
        .route("/manifest.webmanifest", get(manifest))
        .route("/sw.js", get(service_worker))
        .route("/api/auth/request-code", post(request_code))
        .route("/api/auth/verify-code", post(verify_code))
        .route("/api/auth/register", post(register))
        .route("/api/auth/login", post(login))
        .route("/api/auth/logout", post(logout))
        .route("/api/auth/me", get(me))
        .route("/api/auth/socket-token", get(socket_token))
        .route("/api/auth/profile", get(profile))
        .route("/api/auth/saved-games", get(saved_games))
        .route("/api/auth/friends", get(friends))
        .route("/api/auth/search-users", get(search_users))
        .route("/api/leaderboard", get(leaderboard))
        .route("/api/daily", get(daily_info))
        .route("/api/daily/leaderboard", get(daily_leaderboard))
        .route("/api/announcements", get(announcements))
        .route("/api/push/public-key", get(push_public_key))
        .route("/api/push/subscribe", post(push_subscribe))
        .route("/api/push/unsubscribe", post(push_unsubscribe))
        .route("/api/practice/quiz", get(practice_quiz))
        .route("/api/practice/answer", post(practice_answer))
        .route("/api/practice/short-words", get(practice_short_words))
        .route("/api/practice/rack", get(practice_rack))
        .route("/api/practice/rack-word", post(practice_rack_word))
        .route("/api/practice/word-review", get(word_review_next).post(word_review_vote))
        .route("/api/practice/word-review/undo", post(word_review_undo))
        .route("/api/practice/word-review/stats", get(word_review_stats))
        .route("/api/dictionary/check", get(dictionary_check).post(dictionary_check))
        .route("/api/game/{game_id}/share", post(share_game))
        .route("/api/replay/{token}", get(shared_replay))
        .route("/api/game/{game_id}/moves", get(game_moves))
        .route("/api/game/{game_id}/analysis", get(game_analysis))
        .route("/api/async/games", get(async_games_list))
        .route("/api/game/{game_id}/abandon", post(abandon_game))
}
