//! Admin API végpontok (a Python `admin_api_*.py` fájlok portja). A logika az `admin::*` moduloké; itt csak a
//! kérés → függvényhívás → válasz összekötés van (és ahol az élő állapot kell, annak zárolása).

mod comm;
mod dict;
mod game;
mod system;
mod users;

use super::routes::AdminRouter;
use crate::state::ServerState;
use std::collections::HashSet;

pub fn register_all(r: &mut AdminRouter) {
    users::register(r);
    game::register(r);
    dict::register(r);
    comm::register(r);
    system::register(r);
}

/// A most futó szobákhoz tartozó adatbázis-játékazonosítók (ezeket az archívum nem törölheti).
pub(super) fn live_game_ids(st: &ServerState) -> HashSet<i64> {
    st.rooms.values().filter_map(|room| room.db_game_id).collect()
}
