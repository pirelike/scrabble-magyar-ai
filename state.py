import secrets

from room import generate_join_code


class ServerState:
    """Szerver szintű állapot egyetlen objektumban.

    Egyetlen helyen kezeli az összes dict-et, így a szinkronizáció
    (pl. játékos kilépéskor 5-6 dict frissítése) nem duplikálódik.
    """

    def __init__(self):
        # Aktív szobák: {room_id: Room}
        self.rooms = {}
        # join_code -> room_id mapping (gyors keresés)
        self.join_codes = {}
        # Játékos -> szoba mapping: {sid: room_id}
        self.player_rooms = {}
        # Játékos nevek: {sid: name}
        self.player_names = {}
        # Játékos auth info: {sid: {user_id, is_guest}}
        self.player_auth = {}
        # Reconnect tokenek: {token: {room_id, player_name, sid}}
        self._reconnect_tokens = {}
        # Sid -> reconnect token mapping: {sid: token}
        self._sid_to_token = {}
        # Disconnected játékosok grace period: {token: {room_id, sid, player_name}}
        self._disconnected_players = {}

    # --- Player lifecycle ---

    def register_player(self, sid, name, auth_info):
        """Játékos név és auth info regisztrálása (set_name)."""
        self.player_names[sid] = name
        self.player_auth[sid] = auth_info

    def unregister_player(self, sid):
        """Játékos összes adatának törlése (disconnect végén).

        Törli a nevet, auth infót és az esetleges reconnect tokent.
        NEM törli a player_rooms-t — azt a hívó kezeli.
        """
        self.player_names.pop(sid, None)
        self.player_auth.pop(sid, None)
        token = self._sid_to_token.pop(sid, None)
        if token:
            self._reconnect_tokens.pop(token, None)

    # --- Reconnection ---

    def generate_reconnect_token(self, sid, room_id, player_name):
        """Reconnect token generálása és mentése. Visszaadja a tokent."""
        token = secrets.token_urlsafe(24)
        self._reconnect_tokens[token] = {
            'room_id': room_id,
            'player_name': player_name,
            'sid': sid,
        }
        self._sid_to_token[sid] = token
        return token

    def get_reconnect_token_for_sid(self, sid):
        """Visszaadja az SID-hez tartozó reconnect tokent, vagy None."""
        return self._sid_to_token.get(sid)

    def get_disconnected_info(self, token):
        """Visszaadja a disconnected player info-t, vagy None."""
        return self._disconnected_players.get(token)

    def get_token_info(self, token):
        """Visszaadja a reconnect token info-t, vagy None."""
        return self._reconnect_tokens.get(token)

    def mark_disconnected(self, token, sid, room_id, player_name):
        """Játékos ideiglenesen lecsatlakozottnak jelölése (grace period indítás)."""
        self._disconnected_players[token] = {
            'room_id': room_id,
            'sid': sid,
            'player_name': player_name,
        }

    def complete_rejoin(self, token, new_sid):
        """Token alapú újracsatlakozás: dict-ek frissítése.

        Visszaadja a disconnected info-t (room_id, sid, player_name), vagy None.
        """
        dc_info = self._disconnected_players.pop(token, None)
        if not dc_info:
            return None

        old_sid = dc_info['sid']
        self._reconnect_tokens[token]['sid'] = new_sid
        self._sid_to_token.pop(old_sid, None)
        self._sid_to_token[new_sid] = token

        return dc_info

    def finalize_disconnect(self, token):
        """Grace period lejárt: disconnected player info törlése.

        Visszaadja a disconnected info-t, vagy None.
        """
        info = self._disconnected_players.pop(token, None)
        if info:
            self._reconnect_tokens.pop(token, None)
            self._sid_to_token.pop(info['sid'], None)
        return info

    def cleanup_player_token(self, sid):
        """Egy játékos tokenjének és disconnect info-jának törlése."""
        token = self._sid_to_token.pop(sid, None)
        if token:
            self._reconnect_tokens.pop(token, None)
            self._disconnected_players.pop(token, None)

    # --- Room lifecycle ---

    def add_room(self, room):
        """Szoba hozzáadása a rooms és join_codes dict-hez."""
        self.rooms[room.id] = room
        self.join_codes[room.join_code] = room.id

    def remove_room(self, room_id):
        """Szoba és a hozzá tartozó join code törlése."""
        room = self.rooms.pop(room_id, None)
        if room and room.join_code in self.join_codes:
            del self.join_codes[room.join_code]

    def cleanup_room_tokens(self, room_id):
        """Szobához tartozó összes disconnected player és token törlése."""
        tokens_to_remove = [
            t for t, info in self._disconnected_players.items()
            if info['room_id'] == room_id
        ]
        for t in tokens_to_remove:
            dc = self._disconnected_players.pop(t)
            self._sid_to_token.pop(dc['sid'], None)
            self._reconnect_tokens.pop(t, None)

    def cleanup_room(self, room_id):
        """Szoba teljes erőforrásainak felszabadítása: tokenek + szoba törlés."""
        self.cleanup_room_tokens(room_id)
        self.remove_room(room_id)

    def get_room_for_player(self, sid):
        """Szoba lekérdezése SID alapján.

        Visszatér: (room_id, Room, Game) vagy (None, None, None).
        """
        room_id = self.player_rooms.get(sid)
        if not room_id or room_id not in self.rooms:
            return None, None, None
        room = self.rooms[room_id]
        return room_id, room, room.game

    def get_rooms_list(self):
        """Nyilvános szobák listája a lobby számára (restored/private/befejezett kiszűrve)."""
        return [
            room.to_lobby_dict()
            for room in self.rooms.values()
            if room.is_lobby_visible
        ]

    def generate_join_code(self):
        """Egyedi 6-jegyű csatlakozási kód generálása."""
        return generate_join_code(self.join_codes)
