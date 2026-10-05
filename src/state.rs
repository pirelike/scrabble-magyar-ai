//! Szerver szintű állapot egyetlen objektumban: szobák, játékosok, tokenek, újracsatlakozás, megfigyelők, online
//! felhasználók és meghívók. Egyetlen helyen kezeli az összes táblát, így a szinkronizáció (pl. játékos kilépéskor
//! 5-6 tábla frissítése) nem duplikálódik.

use crate::db::Db;
use crate::room::{Room, generate_join_code};
use crate::util;
use std::collections::{HashMap, HashSet};

/// Egy kapcsolat azonossága: regisztrált felhasználó vagy vendég.
#[derive(Clone, Debug, PartialEq)]
pub struct AuthInfo {
    pub user_id: Option<i64>,
    pub is_guest: bool,
}

impl AuthInfo {
    pub fn guest() -> AuthInfo {
        AuthInfo { user_id: None, is_guest: true }
    }

    /// A regisztrált felhasználó azonosítója (vendégnél None).
    pub fn registered_id(&self) -> Option<i64> {
        if self.is_guest { None } else { self.user_id.filter(|u| *u != 0) }
    }
}

#[derive(Clone, Debug)]
pub struct TokenInfo {
    pub room_id: String,
    pub player_name: String,
    pub sid: String,
    pub auth_info: Option<AuthInfo>,
}

#[derive(Clone, Debug)]
pub struct DisconnectedInfo {
    pub room_id: String,
    pub sid: String,
    pub player_name: String,
    pub auth_info: Option<AuthInfo>,
    pub seq: u64,
    pub at: f64,
}

#[derive(Clone, Debug)]
pub struct Invite {
    pub from_user_id: i64,
    pub to_user_id: i64,
    pub room_id: String,
    pub from_sid: String,
}

#[derive(Default)]
pub struct ServerState {
    /// aktív szobák: {room_id: Room}
    pub rooms: HashMap<String, Room>,
    /// join_code -> room_id (gyors keresés)
    pub join_codes: HashMap<String, String>,
    /// játékos -> szoba: {sid: room_id}
    pub player_rooms: HashMap<String, String>,
    pub player_names: HashMap<String, String>,
    pub player_auth: HashMap<String, AuthInfo>,
    /// reconnect tokenek: {token: info}
    pub reconnect_tokens: HashMap<String, TokenInfo>,
    pub sid_to_token: HashMap<String, String>,
    /// lecsatlakozott játékosok türelmi ideje: {token: info}
    pub disconnected_players: HashMap<String, DisconnectedInfo>,
    /// Minden lecsatlakozás kap egy sorszámot, hogy a régi türelmi idő lejárta ne a későbbi (újabb)
    /// lecsatlakozást zárja le.
    disconnect_seq: u64,
    /// megfigyelők: {sid: room_id}
    pub spectator_rooms: HashMap<String, String>,
    /// háttérbe került (nem látható) böngészőlapok: a push értesítés ilyenkor is elmegy
    pub hidden_sids: HashSet<String>,
    /// az admin panel élő kapcsolatai: {sid: user_id} (nem számítanak online felhasználónak)
    pub admin_sids: HashMap<String, i64>,
    /// online regisztrált felhasználók
    online_users: HashMap<i64, HashSet<String>>,
    sid_to_user_id: HashMap<String, i64>,
    pending_invites: HashMap<i64, Invite>,
    invite_counter: i64,
}

impl ServerState {
    pub fn new() -> ServerState {
        ServerState::default()
    }

    // --- Player lifecycle ---

    /// Játékos név és auth info regisztrálása (set_name).
    pub fn register_player(&mut self, sid: &str, name: &str, auth_info: AuthInfo) {
        // Ha ugyanaz a kapcsolat másik felhasználóként lép be (kijelentkezés után), az előző azonosság ne maradjon
        // "online" bejegyzésként.
        self.remove_online_user(sid);
        self.player_names.insert(sid.to_string(), name.to_string());
        if let Some(user_id) = auth_info.registered_id() {
            self.sid_to_user_id.insert(sid.to_string(), user_id);
            self.online_users.entry(user_id).or_default().insert(sid.to_string());
        }
        self.player_auth.insert(sid.to_string(), auth_info);
    }

    /// Játékos összes adatának törlése (disconnect végén): név, auth info és az esetleges reconnect token. NEM törli
    /// a player_rooms-t — azt a hívó kezeli.
    pub fn unregister_player(&mut self, sid: &str) {
        self.player_names.remove(sid);
        self.player_auth.remove(sid);
        if let Some(token) = self.sid_to_token.remove(sid) {
            self.reconnect_tokens.remove(&token);
        }
        self.remove_online_user(sid);
    }

    // --- Reconnection ---

    /// Reconnect token generálása és mentése. Regisztrált játékosnál a felhasználóhoz kötött, tartós token
    /// (adatbázisból), vendégnél kriptográfiailag erős véletlen token.
    pub fn generate_reconnect_token(&mut self, db: &Db, sid: &str, room_id: &str, player_name: &str, auth_info: Option<&AuthInfo>) -> String {
        let user_id = auth_info.and_then(|a| a.registered_id());
        let token = match user_id {
            Some(uid) => db.get_or_create_user_reconnect_token(uid).unwrap_or_else(|_| util::token_urlsafe(16)),
            None => loop {
                let t = util::token_urlsafe(16);
                if !self.reconnect_tokens.contains_key(&t) {
                    break t;
                }
            },
        };
        // Régi bejegyzés takarítása (ha a token már létezik más szobához)
        if let Some(old) = self.reconnect_tokens.get(&token) {
            let old_sid = old.sid.clone();
            self.sid_to_token.remove(&old_sid);
            self.disconnected_players.remove(&token);
        }
        self.reconnect_tokens.insert(
            token.clone(),
            TokenInfo { room_id: room_id.to_string(), player_name: player_name.to_string(), sid: sid.to_string(), auth_info: auth_info.cloned() },
        );
        self.sid_to_token.insert(sid.to_string(), token.clone());
        token
    }

    pub fn get_reconnect_token_for_sid(&self, sid: &str) -> Option<String> {
        self.sid_to_token.get(sid).cloned()
    }

    pub fn get_disconnected_info(&self, token: &str) -> Option<&DisconnectedInfo> {
        self.disconnected_players.get(token)
    }

    pub fn get_token_info(&self, token: &str) -> Option<&TokenInfo> {
        self.reconnect_tokens.get(token)
    }

    /// Játékos ideiglenesen lecsatlakozottnak jelölése (türelmi idő indítása). Visszaadja a lecsatlakozás
    /// sorszámát: a türelmi idő lejártakor csak akkor szabad a játékost véglegesen eltávolítani, ha még ugyanez a
    /// lecsatlakozás van érvényben (`disconnect_is_current`).
    pub fn mark_disconnected(&mut self, token: &str, sid: &str, room_id: &str, player_name: &str, auth_info: Option<AuthInfo>) -> u64 {
        self.disconnect_seq += 1;
        self.disconnected_players.insert(
            token.to_string(),
            DisconnectedInfo {
                room_id: room_id.to_string(),
                sid: sid.to_string(),
                player_name: player_name.to_string(),
                auth_info,
                seq: self.disconnect_seq,
                at: util::now(),
            },
        );
        self.disconnect_seq
    }

    /// Igaz, ha a tokenhez még a `seq` sorszámú lecsatlakozás tartozik (nem csatlakozott vissza, és nem is szakadt
    /// meg újra azóta).
    pub fn disconnect_is_current(&self, token: &str, seq: u64) -> bool {
        self.disconnected_players.get(token).is_some_and(|info| info.seq == seq)
    }

    /// Token alapú újracsatlakozás: táblák frissítése. Visszaadja a disconnected info-t, vagy None.
    pub fn complete_rejoin(&mut self, token: &str, new_sid: &str) -> Option<DisconnectedInfo> {
        let info = self.disconnected_players.remove(token)?;
        if let Some(entry) = self.reconnect_tokens.get_mut(token) {
            entry.sid = new_sid.to_string();
        }
        self.sid_to_token.remove(&info.sid);
        self.sid_to_token.insert(new_sid.to_string(), token.to_string());
        Some(info)
    }

    /// Grace period lejárt: disconnected player info törlése. Visszaadja a disconnected info-t, vagy None.
    pub fn finalize_disconnect(&mut self, token: &str) -> Option<DisconnectedInfo> {
        let info = self.disconnected_players.remove(token)?;
        self.reconnect_tokens.remove(token);
        self.sid_to_token.remove(&info.sid);
        Some(info)
    }

    /// Egy játékos tokenjének és disconnect info-jának törlése.
    pub fn cleanup_player_token(&mut self, sid: &str) {
        if let Some(token) = self.sid_to_token.remove(sid) {
            self.reconnect_tokens.remove(&token);
            self.disconnected_players.remove(&token);
        }
    }

    // --- Room lifecycle ---

    /// Szoba hozzáadása a rooms és join_codes táblához.
    pub fn add_room(&mut self, room: Room) {
        self.join_codes.insert(room.join_code.clone(), room.id.clone());
        self.rooms.insert(room.id.clone(), room);
    }

    /// Szoba és a hozzá tartozó join code törlése.
    pub fn remove_room(&mut self, room_id: &str) {
        if let Some(room) = self.rooms.remove(room_id) {
            if self.join_codes.get(&room.join_code).is_some_and(|id| id == room_id) {
                self.join_codes.remove(&room.join_code);
            }
        }
    }

    /// Szobához tartozó összes disconnected player és token törlése.
    pub fn cleanup_room_tokens(&mut self, room_id: &str) {
        let tokens: Vec<String> = self.disconnected_players.iter().filter(|(_, i)| i.room_id == room_id).map(|(t, _)| t.clone()).collect();
        for token in tokens {
            if let Some(dc) = self.disconnected_players.remove(&token) {
                self.sid_to_token.remove(&dc.sid);
                self.reconnect_tokens.remove(&token);
            }
        }
    }

    /// Szoba teljes erőforrásainak felszabadítása: meghívók + tokenek + megfigyelők + szoba törlés.
    pub fn cleanup_room(&mut self, room_id: &str) {
        self.remove_invites_for_room(room_id);
        self.cleanup_room_tokens(room_id);
        self.spectator_rooms.retain(|_, rid| rid != room_id);
        self.remove_room(room_id);
    }

    /// Szoba lekérdezése SID alapján: a szoba azonosítója (ha a játékos szobában van és a szoba létezik).
    pub fn room_id_for_player(&self, sid: &str) -> Option<String> {
        let room_id = self.player_rooms.get(sid)?;
        if self.rooms.contains_key(room_id) { Some(room_id.clone()) } else { None }
    }

    /// Nyilvános szobák listája a lobby számára (restored/private/befejezett kiszűrve).
    pub fn get_rooms_list(&self) -> Vec<serde_json::Value> {
        self.rooms_in_creation_order().into_iter().filter(|r| r.is_lobby_visible()).map(|r| r.to_lobby_dict()).collect()
    }

    /// Nyilvános, folyamatban lévő játékok listája (megfigyeléshez).
    pub fn get_live_games(&self) -> Vec<serde_json::Value> {
        self.rooms_in_creation_order().into_iter().filter(|r| r.is_live_visible()).map(|r| r.to_live_dict()).collect()
    }

    /// A szobák létrehozásuk sorrendjében (a Python szótár beszúrási sorrendje): a listák így stabilak.
    pub fn rooms_in_creation_order(&self) -> Vec<&Room> {
        let mut rooms: Vec<&Room> = self.rooms.values().collect();
        rooms.sort_by(|a, b| a.created_at.partial_cmp(&b.created_at).unwrap_or(std::cmp::Ordering::Equal).then_with(|| a.id.cmp(&b.id)));
        rooms
    }

    // --- Megfigyelők ---

    pub fn add_spectator(&mut self, sid: &str, room_id: &str) {
        self.spectator_rooms.insert(sid.to_string(), room_id.to_string());
    }

    /// Megfigyelő eltávolítása. Visszatér: a szoba azonosítója, vagy None.
    pub fn remove_spectator(&mut self, sid: &str) -> Option<String> {
        let room_id = self.spectator_rooms.remove(sid)?;
        if let Some(room) = self.rooms.get_mut(&room_id) {
            room.spectators.remove(sid);
        }
        Some(room_id)
    }

    /// Az SID által megfigyelt szoba azonosítója, vagy None.
    pub fn get_spectated_room(&self, sid: &str) -> Option<String> {
        self.spectator_rooms.get(sid).cloned()
    }

    /// Egyedi 6-jegyű csatlakozási kód generálása.
    pub fn generate_join_code(&self) -> String {
        generate_join_code(&self.join_codes)
    }

    // --- Online tracking & Invites ---

    /// Online tracking eltávolítása SID alapján (disconnect-kor).
    pub fn remove_online_user(&mut self, sid: &str) {
        if let Some(user_id) = self.sid_to_user_id.remove(sid) {
            if let Some(sids) = self.online_users.get_mut(&user_id) {
                sids.remove(sid);
                if sids.is_empty() {
                    self.online_users.remove(&user_id);
                }
            }
        }
    }

    /// Online tracking visszaállítása (újracsatlakozáskor).
    pub fn add_online_user(&mut self, sid: &str, user_id: i64) {
        self.sid_to_user_id.insert(sid.to_string(), user_id);
        self.online_users.entry(user_id).or_default().insert(sid.to_string());
    }

    pub fn is_user_online(&self, user_id: i64) -> bool {
        self.online_users.contains_key(&user_id)
    }

    /// A felhasználó aktív SID-jei.
    pub fn get_user_sids(&self, user_id: i64) -> Vec<String> {
        self.online_users.get(&user_id).map(|s| s.iter().cloned().collect()).unwrap_or_default()
    }

    pub fn get_online_user_ids(&self) -> HashSet<i64> {
        self.online_users.keys().copied().collect()
    }

    pub fn online_user_count(&self) -> usize {
        self.online_users.len()
    }

    pub fn get_user_id_for_sid(&self, sid: &str) -> Option<i64> {
        self.sid_to_user_id.get(sid).copied()
    }

    /// Meghívás létrehozása.
    pub fn create_invite(&mut self, from_uid: i64, to_uid: i64, room_id: &str, from_sid: &str) -> i64 {
        self.invite_counter += 1;
        self.pending_invites.insert(
            self.invite_counter,
            Invite { from_user_id: from_uid, to_user_id: to_uid, room_id: room_id.to_string(), from_sid: from_sid.to_string() },
        );
        self.invite_counter
    }

    pub fn get_invite(&self, invite_id: i64) -> Option<&Invite> {
        self.pending_invites.get(&invite_id)
    }

    pub fn remove_invite(&mut self, invite_id: i64) -> Option<Invite> {
        self.pending_invites.remove(&invite_id)
    }

    /// Szoba összes meghívásának törlése.
    pub fn remove_invites_for_room(&mut self, room_id: &str) {
        self.pending_invites.retain(|_, inv| inv.room_id != room_id);
    }

    /// Felhasználónak szóló aktív meghívások.
    pub fn get_invites_for_user(&self, user_id: i64) -> Vec<(i64, Invite)> {
        self.pending_invites.iter().filter(|(_, inv)| inv.to_user_id == user_id).map(|(id, inv)| (*id, inv.clone())).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::Game;

    fn auth(user_id: i64) -> AuthInfo {
        AuthInfo { user_id: Some(user_id), is_guest: false }
    }

    fn room(id: &str, code: &str) -> Room {
        Room::new(id, Game::with_defaults(id), Some("sid"), "A", "Szoba", 4, code, false, None)
    }

    #[test]
    fn register_and_unregister_tracks_online_users() {
        let mut st = ServerState::new();
        st.register_player("s1", "Alice", auth(7));
        st.register_player("s2", "Alice", auth(7));
        st.register_player("g1", "Vendég", AuthInfo::guest());
        assert!(st.is_user_online(7));
        assert_eq!(st.get_user_sids(7).len(), 2);
        assert!(!st.is_user_online(0));
        st.unregister_player("s1");
        assert!(st.is_user_online(7));
        st.unregister_player("s2");
        assert!(!st.is_user_online(7));
        assert_eq!(st.get_user_id_for_sid("g1"), None);
    }

    #[test]
    fn re_registering_as_another_user_drops_the_old_identity() {
        let mut st = ServerState::new();
        st.register_player("s1", "Alice", auth(7));
        st.register_player("s1", "Béla", auth(8));
        assert!(!st.is_user_online(7));
        assert!(st.is_user_online(8));
    }

    #[test]
    fn disconnect_sequence_protects_newer_disconnects() {
        let mut st = ServerState::new();
        let first = st.mark_disconnected("tok", "s1", "r", "A", None);
        assert!(st.disconnect_is_current("tok", first));
        st.reconnect_tokens.insert("tok".into(), TokenInfo { room_id: "r".into(), player_name: "A".into(), sid: "s1".into(), auth_info: None });
        st.sid_to_token.insert("s1".into(), "tok".into());
        let info = st.complete_rejoin("tok", "s2").unwrap();
        assert_eq!(info.sid, "s1");
        assert!(!st.disconnect_is_current("tok", first));
        let second = st.mark_disconnected("tok", "s2", "r", "A", None);
        assert_ne!(first, second);
        assert!(!st.disconnect_is_current("tok", first), "a régi lejárat nem érinti az újabb lecsatlakozást");
        assert!(st.disconnect_is_current("tok", second));
        assert!(st.finalize_disconnect("tok").is_some());
        assert!(st.finalize_disconnect("tok").is_none());
        assert!(st.get_token_info("tok").is_none());
    }

    #[test]
    fn rooms_and_join_codes() {
        let mut st = ServerState::new();
        st.add_room(room("r1", "111111"));
        st.add_room(room("r2", "222222"));
        assert_eq!(st.join_codes["111111"], "r1");
        assert_eq!(st.get_rooms_list().len(), 2);
        st.remove_room("r1");
        assert!(!st.join_codes.contains_key("111111"));
        assert_eq!(st.rooms.len(), 1);
    }

    #[test]
    fn cleanup_room_removes_everything_attached() {
        let mut st = ServerState::new();
        st.add_room(room("r1", "111111"));
        st.spectator_rooms.insert("sp".into(), "r1".into());
        st.mark_disconnected("tok", "s1", "r1", "A", None);
        st.sid_to_token.insert("s1".into(), "tok".into());
        st.reconnect_tokens.insert("tok".into(), TokenInfo { room_id: "r1".into(), player_name: "A".into(), sid: "s1".into(), auth_info: None });
        let invite = st.create_invite(1, 2, "r1", "s1");
        st.cleanup_room("r1");
        assert!(st.rooms.is_empty() && st.spectator_rooms.is_empty() && st.disconnected_players.is_empty());
        assert!(st.reconnect_tokens.is_empty() && st.sid_to_token.is_empty());
        assert!(st.get_invite(invite).is_none());
        assert!(st.join_codes.is_empty());
    }

    #[test]
    fn invites() {
        let mut st = ServerState::new();
        let a = st.create_invite(1, 2, "r", "s");
        let b = st.create_invite(1, 3, "r", "s");
        assert!(b > a);
        assert_eq!(st.get_invites_for_user(2).len(), 1);
        assert_eq!(st.remove_invite(a).unwrap().to_user_id, 2);
        assert!(st.remove_invite(a).is_none());
        st.remove_invites_for_room("r");
        assert!(st.get_invite(b).is_none());
    }

    #[test]
    fn user_reconnect_token_is_persistent_for_registered_users() {
        let db = Db::open_temp().unwrap();
        let uid = db.create_user("a@example.com", "A", "pass123").unwrap();
        let mut st = ServerState::new();
        let t1 = st.generate_reconnect_token(&db, "s1", "r1", "A", Some(&auth(uid)));
        let t2 = st.generate_reconnect_token(&db, "s2", "r2", "A", Some(&auth(uid)));
        assert_eq!(t1, t2);
        assert_eq!(st.reconnect_tokens[&t1].room_id, "r2");
        let g1 = st.generate_reconnect_token(&db, "g1", "r3", "G", Some(&AuthInfo::guest()));
        let g2 = st.generate_reconnect_token(&db, "g2", "r3", "G", None);
        assert_ne!(g1, g2);
    }

    #[test]
    fn spectators_are_tracked_per_room() {
        let mut st = ServerState::new();
        let mut r = room("r1", "111111");
        r.spectators.insert("sp".into(), "Néző".into());
        st.add_room(r);
        st.add_spectator("sp", "r1");
        assert_eq!(st.get_spectated_room("sp").as_deref(), Some("r1"));
        assert_eq!(st.remove_spectator("sp").as_deref(), Some("r1"));
        assert!(st.rooms["r1"].spectators.is_empty());
        assert!(st.remove_spectator("sp").is_none());
    }
}
