//! A szerver: Socket.IO eseménykezelők, HTTP útvonalak, háttérfolyamatok.

pub mod core;
pub mod events;
pub mod extras;
pub mod http;
pub mod net;
pub mod play;

use crate::app::App;
use axum::extract::ConnectInfo;
use serde_json::Value;
use socketioxide::extract::{SocketRef, State, TryData};
use socketioxide::layer::SocketIoLayer;
use socketioxide::SocketIo;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

/// Egy szinkron eseménykezelő bekötése: a belépési pont zárolja a szerver állapotát, a kezelő a zárolt állapoton
/// dolgozik (a Python változat gevent alatt hasonlóan egyetlen szálon futott).
macro_rules! sync_event {
    ($socket:expr, $name:literal, $handler:path) => {
        $socket.on(
            $name,
            async |s: SocketRef, State(app): State<Arc<App>>, TryData(data): TryData<Value>| {
                let sid = s.id.to_string();
                let data = data.unwrap_or(Value::Null);
                let mut st = app.state.lock();
                $handler(&app, &mut st, &sid, data);
            },
        );
    };
}

/// A Socket.IO kapcsolat kliens IP-je (a handshake kérésből).
fn socket_ip(socket: &SocketRef) -> String {
    let parts = socket.req_parts();
    let peer = parts.extensions.get::<ConnectInfo<SocketAddr>>().map(|c| c.0.ip());
    net::client_ip(&parts.headers, peer)
}

/// Kitiltott IP-ről a kapcsolat el sem jön létre.
async fn ip_guard(socket: SocketRef, State(app): State<Arc<App>>) -> Result<(), String> {
    if app.ip_bans.is_banned(&app.db, &socket_ip(&socket)) {
        return Err("forbidden".to_string());
    }
    Ok(())
}

async fn on_connect(socket: SocketRef) {
    sync_event!(socket, "set_name", events::set_name);
    sync_event!(socket, "logout", events::logout);
    sync_event!(socket, "rejoin_room", events::rejoin_room);
    sync_event!(socket, "get_rooms", events::get_rooms);
    sync_event!(socket, "create_room", events::create_room);
    sync_event!(socket, "join_room", events::join_room);
    sync_event!(socket, "leave_room", events::leave_room);
    sync_event!(socket, "start_game", events::start_game);
    sync_event!(socket, "save_game", events::save_game);
    sync_event!(socket, "restore_game", events::restore_game);

    sync_event!(socket, "place_tiles", play::place_tiles);
    sync_event!(socket, "exchange_tiles", play::exchange_tiles);
    sync_event!(socket, "pass_turn", play::pass_turn);
    sync_event!(socket, "accept_words", play::accept_words);
    sync_event!(socket, "reject_words", play::reject_words);
    sync_event!(socket, "withdraw_words", play::withdraw_words);
    sync_event!(socket, "send_chat", play::send_chat);
    sync_event!(socket, "report_content", play::report_content);
    sync_event!(socket, "preview_move", play::preview_move);
    sync_event!(socket, "set_visibility", play::set_visibility);

    sync_event!(socket, "retry_daily", extras::retry_daily);
    sync_event!(socket, "reveal_daily", extras::reveal_daily);
    sync_event!(socket, "create_async_game", extras::create_async_game);
    sync_event!(socket, "open_async_game", extras::open_async_game);
    sync_event!(socket, "resign_game", extras::resign_game);
    sync_event!(socket, "spectate_room", extras::spectate_room);
    sync_event!(socket, "leave_spectate", extras::leave_spectate);
    sync_event!(socket, "send_friend_request", extras::send_friend_request);
    sync_event!(socket, "accept_friend_request", extras::accept_friend_request);
    sync_event!(socket, "decline_friend_request", extras::decline_friend_request);
    sync_event!(socket, "remove_friend", extras::remove_friend);
    sync_event!(socket, "invite_to_room", extras::invite_to_room);
    sync_event!(socket, "respond_invite", extras::respond_invite);
    sync_event!(socket, "admin_subscribe", extras::admin_subscribe);
    sync_event!(socket, "admin_watch_room", extras::admin_watch_room);

    // A hosszabb (háttérszálon számoló) események aszinkron kezelők
    socket.on("request_hint", async |s: SocketRef, State(app): State<Arc<App>>| {
        play::request_hint(app, s.id.to_string()).await;
    });
    socket.on("start_daily", async |s: SocketRef, State(app): State<Arc<App>>| {
        extras::start_daily(app, s.id.to_string()).await;
    });

    socket.on_disconnect(async |s: SocketRef, State(app): State<Arc<App>>| {
        let sid = s.id.to_string();
        let mut st = app.state.lock();
        events::handle_disconnect(&app, &mut st, &sid);
    });
}

/// A Socket.IO réteg és a kibocsátó létrehozása (az alkalmazásba is beállítja).
pub fn build_socketio(app: &Arc<App>) -> (SocketIoLayer, SocketIo) {
    use socketioxide::handler::ConnectHandler;
    let (layer, io) = SocketIo::builder()
        .with_state(app.clone())
        .ping_interval(Duration::from_secs(25))
        .ping_timeout(Duration::from_secs(120))
        .max_payload(1_000_000)
        .max_buffer_size(4096)
        .build_layer();
    io.ns("/", on_connect.with(ip_guard));
    app.set_io(io.clone());
    (layer, io)
}

// ===================================================================================================
// HTTP szolgáltatás és háttérfolyamatok
// ===================================================================================================

use axum::Router;
use axum::extract::Request;
use axum::http::{HeaderValue, StatusCode, header};
use axum::middleware::{Next, from_fn_with_state};
use axum::response::{IntoResponse, Response};
use tower_http::services::ServeDir;
use tower_http::set_header::SetResponseHeaderLayer;

/// A 403-as válasz a kitiltott IP-nek (minden útvonalra ugyanaz, így nem árul el semmit).
fn forbidden() -> Response {
    let body = "<!doctype html>\n<html lang=en>\n<title>403 Forbidden</title>\n<h1>Forbidden</h1>\n<p>You don't have the permission to access the requested resource. It is either read-protected or not readable by the server.</p>\n";
    let mut response = (StatusCode::FORBIDDEN, body).into_response();
    response.headers_mut().insert(header::CONTENT_TYPE, HeaderValue::from_static("text/html; charset=utf-8"));
    response
}

/// Az őr az egész alkalmazás előtt (az útvonal-illesztés előtt): kitiltott IP, majd az admin útvonalak őre.
async fn guard(axum::extract::State(app): axum::extract::State<Arc<App>>, request: Request, next: Next) -> Response {
    let peer = request.extensions().get::<ConnectInfo<SocketAddr>>().map(|c| c.0.ip());
    let ip = net::client_ip(request.headers(), peer);
    if app.ip_bans.is_banned(&app.db, &ip) {
        return forbidden();
    }
    crate::admin::routes::guard(&app, request, next).await
}

/// A teljes HTTP + Socket.IO szolgáltatás.
pub fn build_router(app: &Arc<App>) -> Router {
    let static_files = tower::ServiceBuilder::new()
        .layer(SetResponseHeaderLayer::if_not_present(header::CACHE_CONTROL, HeaderValue::from_static("no-cache")))
        .service(ServeDir::new(app.base_dir.join("static")));
    let (sio_layer, _io) = build_socketio(app);
    let (admin_routes, _table) = crate::admin::routes::admin_router(app);
    let inner = Router::new()
        .merge(http::public_routes())
        .merge(admin_routes)
        .nest_service("/static", static_files)
        .fallback(|| async { http::not_found() })
        .with_state(app.clone())
        .layer(sio_layer);
    Router::new().fallback_service(inner).layer(from_fn_with_state(app.clone(), guard)).layer(axum::middleware::from_fn(finish_response))
}

/// Egységes válaszok: a szöveges típusok `charset=utf-8`-at kapnak (a statikus fájloknál is), az üres törzsű 404 / 405
/// pedig a szokásos oldalt (a nem létező és a nem admin útvonalak így ugyanúgy néznek ki).
async fn finish_response(request: axum::extract::Request, next: axum::middleware::Next) -> axum::response::Response {
    let mut response = next.run(request).await;
    let content_type = response.headers().get(header::CONTENT_TYPE).and_then(|v| v.to_str().ok()).map(|v| v.to_string());
    match content_type {
        None => {
            let replacement = match response.status() {
                StatusCode::NOT_FOUND => http::not_found(),
                StatusCode::METHOD_NOT_ALLOWED => http::method_not_allowed(),
                _ => return response,
            };
            // az eredeti fejlécek (pl. az admin őr biztonsági fejlécei, `Allow`) megmaradnak
            let (mut parts, body) = replacement.into_parts();
            for (name, value) in response.headers() {
                if name != header::CONTENT_LENGTH && name != header::CONTENT_TYPE {
                    parts.headers.insert(name.clone(), value.clone());
                }
            }
            return axum::response::Response::from_parts(parts, body);
        }
        Some(value) => {
            let lower = value.to_ascii_lowercase();
            let textual = lower.starts_with("text/") || lower.contains("javascript");
            if textual && !lower.contains("charset") {
                if let Ok(updated) = HeaderValue::from_str(&format!("{value}; charset=utf-8")) {
                    response.headers_mut().insert(header::CONTENT_TYPE, updated);
                }
            }
        }
    }
    response
}

/// Háttérfolyamatok: levelezős határidők, admin számlálók, ütemezett karbantartás.
pub fn spawn_background_tasks(app: &Arc<App>) {
    let a = app.clone();
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_secs(60)).await;
            let app = a.clone();
            let result = tokio::task::spawn_blocking(move || core::expire_async_turns(&app, None)).await;
            match result {
                Ok(count) => a.job_ran("async_sweeper", true, Some(count.to_string())),
                Err(e) => {
                    a.job_ran("async_sweeper", false, Some(e.to_string()));
                    println!("[async] Hiba a határidők ellenőrzésénél: {e}");
                }
            }
        }
    });
    let a = app.clone();
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_secs(5)).await;
            if a.room_member_count(core::ADMIN_ROOM) > 0 {
                let overview = {
                    let st = a.state.lock();
                    crate::admin::live::overview_light(&a, &st)
                };
                a.emit_room(core::ADMIN_ROOM, "admin_overview", &overview);
            }
            a.job_ran("admin_broadcaster", true, None);
        }
    });
    // Óránként: napi mentés (ha be van kapcsolva) és a régi chat napló törlése
    let a = app.clone();
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_secs(3600)).await;
            let app = a.clone();
            match tokio::task::spawn_blocking(move || crate::admin::system::run_scheduled_maintenance(&app)).await {
                Ok(done) => a.job_ran("maintenance_tasks", true, (!done.is_empty()).then(|| done.join(","))),
                Err(e) => {
                    a.job_ran("maintenance_tasks", false, Some(e.to_string()));
                    println!("[admin] Hiba az ütemezett karbantartásnál: {e}");
                }
            }
        }
    });
}

/// A szerver indítása a megadott porton (a folyamat leállásáig fut).
pub async fn serve(app: Arc<App>, port: u16) -> std::io::Result<()> {
    let listener = tokio::net::TcpListener::bind(("0.0.0.0", port)).await?;
    println!("  [*] A szerver fut: http://localhost:{port}");
    serve_on(app, listener, shutdown_signal()).await
}

/// A szerver futtatása egy már megnyitott figyelőn, a megadott leállítási jelig (a tesztek ezt használják).
pub async fn serve_on(app: Arc<App>, listener: tokio::net::TcpListener, shutdown: impl std::future::Future<Output = ()> + Send + 'static) -> std::io::Result<()> {
    let router = build_router(&app);
    axum::serve(listener, router.into_make_service_with_connect_info::<SocketAddr>()).with_graceful_shutdown(shutdown).await
}

async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    #[cfg(unix)]
    let terminate = async {
        if let Ok(mut signal) = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            signal.recv().await;
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();
    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
}
