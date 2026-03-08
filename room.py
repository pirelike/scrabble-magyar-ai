import secrets


class Room:
    """Egy szoba állapota: játék, tulajdonos, beállítások, chat, challenge timer."""

    MAX_CHAT_MESSAGES = 100

    def __init__(self, room_id, game, owner_sid, owner_name, name,
                 max_players, join_code, is_private=False, owner_token=None):
        self.id = room_id
        self.game = game
        self.owner = owner_sid
        self.owner_token = owner_token
        self.owner_name = owner_name
        self.name = name
        self.max_players = max_players
        self.join_code = join_code
        self.is_private = is_private
        self.is_restored = False
        self.chat_messages = []
        self._challenge_timer_id = 0
        self._turn_timer_id = 0
        self.turn_timer_expires_at = None  # float Unix timestamp or None
        # Persistence tracking (korábban a Game-ben volt)
        self.db_game_id = None
        self.last_saved_move_count = 0
        self.manually_saved = False

    def add_chat_message(self, name, message):
        """Chat üzenet hozzáadása (max 100 darab)."""
        self.chat_messages.append({'name': name, 'message': message})
        if len(self.chat_messages) > self.MAX_CHAT_MESSAGES:
            self.chat_messages = self.chat_messages[-self.MAX_CHAT_MESSAGES:]

    def invalidate_challenge_timer(self):
        """Érvényteleníti az aktuális challenge timert. Visszaadja az új timer id-t."""
        self._challenge_timer_id += 1
        return self._challenge_timer_id

    @property
    def challenge_timer_id(self):
        return self._challenge_timer_id

    def invalidate_turn_timer(self):
        """Érvényteleníti az aktuális kör timert. Visszaadja az új timer id-t."""
        self._turn_timer_id += 1
        self.turn_timer_expires_at = None
        return self._turn_timer_id

    @property
    def turn_timer_id(self):
        return self._turn_timer_id

    def transfer_ownership(self, new_owner_sid, new_owner_name, new_owner_token=None):
        """Tulajdonjog átadása."""
        self.owner = new_owner_sid
        self.owner_name = new_owner_name
        self.owner_token = new_owner_token

    @property
    def is_lobby_visible(self):
        """Megjelenik-e a szoba a nyilvános lobby listában."""
        return not self.is_private and not self.is_restored and not self.game.finished and not self.game.started

    def to_lobby_dict(self):
        """Lobby listához szükséges adatok (publikus szobákhoz)."""
        return {
            'id': self.id,
            'name': self.name,
            'players': len(self.game.players),
            'max_players': self.max_players,
            'started': self.game.started,
            'finished': self.game.finished,
            'owner': self.owner_name,
            'challenge_mode': self.game.challenge_mode,
            'turn_time_limit': self.game.turn_time_limit,
            'is_restored': self.is_restored,
        }


def generate_join_code(existing_codes):
    """6 számjegyű egyedi csatlakozási kód generálása."""
    while True:
        code = f'{secrets.randbelow(1000000):06d}'
        if code not in existing_codes:
            return code
