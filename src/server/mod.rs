//! A szerver: Socket.IO eseménykezelők, HTTP útvonalak, háttérfolyamatok.

pub mod core;
pub mod events;
pub mod extras;
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
