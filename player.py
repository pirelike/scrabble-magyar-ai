from ai_player import normalize_level


class Player:
    """Egy játékos állapota."""

    def __init__(self, player_id, name, is_bot=False, difficulty=None):
        self.id = player_id
        self.name = name
        self.hand = []  # Betűzsetonok a kézben
        self.score = 0
        self.skip_next_turn = False  # Challenge büntetés
        self.disconnected = False  # Ideiglenesen lecsatlakozott
        self.is_bot = is_bot  # Számítógépes ellenfél
        # A robot fokozata (1–10); a korábbi 'easy' / 'medium' / 'hard' értékek átképeződnek
        self.difficulty = normalize_level(difficulty) if is_bot else None

    def to_dict(self, reveal_hand=False):
        data = {
            'id': self.id,
            'name': self.name,
            'score': self.score,
            'hand_count': len(self.hand),
            'skip_next_turn': self.skip_next_turn,
            'disconnected': self.disconnected,
            'is_bot': self.is_bot,
            'difficulty': self.difficulty,
        }
        if reveal_hand:
            data['hand'] = self.hand
        return data
