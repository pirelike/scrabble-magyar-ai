import secrets
import time


class Room:
    """Egy szoba állapota: játék, tulajdonos, beállítások, chat, challenge timer."""

    MAX_CHAT_MESSAGES = 100
    MAX_SPECTATORS = 30

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
        self.is_puzzle = False  # napi feladvány (egyjátékos, nem mentődik)
        self.pushed_turn = -1   # melyik körhöz küldtünk már push értesítést
        self.is_async = False   # levelezős játék: tartós, a játékosok bármikor visszatérhetnek
        self.notified_turn = -1  # levelezős játék: melyik körről értesítettük a lobbyban lévő játékost
        self.persisted_sig = None  # levelezős játék: a legutóbb mentett állapot ujjlenyomata
        self.chat_messages = []
        # A chat üzenetek kiegészítő adatai (a `chat_messages` elemei változatlanok): {'ts', 'user_id'} soronként
        self.chat_meta = []
        self.created_at = time.time()
        self.last_activity = self.created_at   # az utolsó állapotküldés ideje (elakadás-jelzéshez)
        self.bot_scheduled_at = None           # a legutóbbi robotlépés-ütemezés ideje, ha van függő
        self.timer_paused_left = None          # szüneteltetett körtimer hátralévő ideje (mp)
        self.admin_watchers = set()            # az élő szobát figyelő admin kapcsolatok (SID)
        self._challenge_timer_id = 0
        self._turn_timer_id = 0
        self._bot_turn_id = 0
        self.spectators = {}  # {sid: név} — megfigyelők
        self.turn_timer_expires_at = None  # float Unix timestamp or None
        self.withdraw_time_left = None  # a szavazásra váró lerakáskor hátralévő köridő (visszavonáshoz)
        # Persistence tracking (korábban a Game-ben volt)
        self.db_game_id = None
        self.last_saved_move_count = 0
        self.manually_saved = False
        self.result_saved = False  # a végeredmény (statisztika) már rögzítve van
        # Visszaállított játék: név -> user_id (None = vendég) a mentésből
        self.known_user_ids = {}
        # Visszaállított játékból hiányzó játékosok, akik menet közben még csatlakozhatnak
        self.late_join_names = {}

    def add_chat_message(self, name, message, user_id=None, system=False):
        """Chat üzenet hozzáadása (max 100 darab). `system`: rendszerüzenet (kiemelten jelenik meg)."""
        entry = {'name': name, 'message': message}
        if system:
            entry['system'] = True
        self.chat_messages.append(entry)
        self.chat_meta.append({'ts': time.time(), 'user_id': user_id})
        if len(self.chat_messages) > self.MAX_CHAT_MESSAGES:
            self.chat_messages = self.chat_messages[-self.MAX_CHAT_MESSAGES:]
            self.chat_meta = self.chat_meta[-self.MAX_CHAT_MESSAGES:]

    def touch(self):
        """Az utolsó tevékenység ideje (állapotküldéskor frissül)."""
        self.last_activity = time.time()

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

    def invalidate_bot_turn(self):
        """Érvényteleníti a függőben lévő robotlépést. Visszaadja az új azonosítót."""
        self._bot_turn_id += 1
        return self._bot_turn_id

    @property
    def bot_turn_id(self):
        return self._bot_turn_id

    def transfer_ownership(self, new_owner_sid, new_owner_name, new_owner_token=None):
        """Tulajdonjog átadása."""
        self.owner = new_owner_sid
        self.owner_name = new_owner_name
        self.owner_token = new_owner_token

    @property
    def is_lobby_visible(self):
        """Megjelenik-e a szoba a nyilvános lobby listában."""
        return not self.is_private and not self.is_restored and not self.game.finished and not self.game.started

    @property
    def is_live_visible(self):
        """Megfigyelhető-e a lobby "Élő játékok" listájából (nyilvános, folyamatban lévő játék)."""
        return not self.is_private and self.game.started and not self.game.finished

    def to_live_dict(self):
        """Az "Élő játékok" listához szükséges adatok."""
        return {
            'id': self.id,
            'name': self.name,
            'players': [
                {'name': p.name, 'score': p.score, 'is_bot': p.is_bot}
                for p in self.game.players
            ],
            'challenge_mode': self.game.challenge_mode,
            'turn_time_limit': self.game.turn_time_limit,
            'spectators': len(self.spectators),
            'turn_number': self.game.turn_number,
        }

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
            'bots': sum(1 for p in self.game.players if p.is_bot),
        }


def generate_join_code(existing_codes):
    """6 számjegyű egyedi csatlakozási kód generálása."""
    while True:
        code = f'{secrets.randbelow(1000000):06d}'
        if code not in existing_codes:
            return code
