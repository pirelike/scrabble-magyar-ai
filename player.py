class Player:
    """Egy játékos állapota."""

    def __init__(self, player_id, name):
        self.id = player_id
        self.name = name
        self.hand = []  # Betűzsetonok a kézben
        self.score = 0
        self.consecutive_passes = 0
        self.skip_next_turn = False  # Challenge büntetés
        self.disconnected = False  # Ideiglenesen lecsatlakozott

    def to_dict(self, reveal_hand=False):
        data = {
            'id': self.id,
            'name': self.name,
            'score': self.score,
            'hand_count': len(self.hand),
            'skip_next_turn': self.skip_next_turn,
            'disconnected': self.disconnected,
        }
        if reveal_hand:
            data['hand'] = self.hand
        return data
