//! Admin panel: élő szobák (kezdetleges változat — a teljes a későbbi lépésben készül).

use crate::app::App;
use crate::room::Room;
use crate::state::ServerState;
use serde_json::{Value, json};
use std::sync::Arc;

/// Egy szoba sora az élő listában.
pub fn room_summary(room: &Room) -> Value {
    json!({"id": room.id, "name": room.name})
}

/// Egy szoba teljes állapota az admin panelnek.
pub fn room_detail(_app: &Arc<App>, st: &ServerState, room_id: &str) -> Value {
    match st.rooms.get(room_id) {
        Some(room) => room_summary(room),
        None => Value::Null,
    }
}

/// Az élő számlálók (az `admin` szobának ötmásodpercenként).
pub fn overview_light(_app: &Arc<App>, st: &ServerState) -> Value {
    json!({
        "online": st.online_user_count(),
        "rooms": st.rooms.len(),
    })
}
