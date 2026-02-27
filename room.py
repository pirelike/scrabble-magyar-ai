import secrets


class Room:
    """Egy szoba állapota: játék, tulajdonos, beállítások, chat, challenge timer."""

    MAX_CHAT_MESSAGES = 100

    def __init__(self, room_id, game, owner_sid, owner_name, name,
                 max_players, join_code, is_private=False):
        self.id = room_id
        self.game = game
        self.owner = owner_sid
        self.owner_name = owner_name
        self.name = name
        self.max_players = max_players
        self.join_code = join_code
        self.is_private = is_private
        self.chat_messages = []
        self._challenge_timer_id = 0

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

    def transfer_ownership(self, new_owner_sid, new_owner_name):
        """Tulajdonjog átadása."""
        self.owner = new_owner_sid
        self.owner_name = new_owner_name

    def to_lobby_dict(self):
        """Lobby listához szükséges adatok (publikus szobákhoz)."""
        return {
            'id': self.id,
            'name': self.name,
            'players': len(self.game.players),
            'max_players': self.max_players,
            'started': self.game.started,
            'owner': self.owner_name,
            'challenge_mode': self.game.challenge_mode,
        }


def generate_join_code(existing_codes):
    """6 számjegyű egyedi csatlakozási kód generálása."""
    while True:
        code = f'{secrets.randbelow(1000000):06d}'
        if code not in existing_codes:
            return code
