//! Admin panel: élő szobák (kezdetleges változat — a teljes a későbbi lépésben készül).

use crate::room::Room;
use serde_json::{Value, json};

/// Egy szoba sora az élő listában.
pub fn room_summary(room: &Room) -> Value {
    json!({"id": room.id, "name": room.name})
}
