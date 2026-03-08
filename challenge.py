import time


class Challenge:
    """Challenge (szavazás) állapot gép.

    Életciklus:
      1. Létrehozás: minden nem-lerakó játékos szavazhat (elfogad/elutasít)
      2. Lezárás: 50%+ elfogadás → elfogadva, különben elutasítva
    """

    def __init__(self, tiles_placed, formed_words, word_strs, score,
                 player_idx, removed_from_hand):
        self.tiles_placed = tiles_placed
        self.formed_words = formed_words
        self.word_strs = word_strs
        self.score = score
        self.player_idx = player_idx
        self.removed_from_hand = removed_from_hand
        self.votes = {}
        self.expires_at = time.time() + 30  # CHALLENGE_TIMEOUT

    def add_vote(self, player_id, vote):
        """Szavazat rögzítése."""
        self.votes[player_id] = vote

    def all_voted(self, voter_ids):
        """Mindenki szavazott-e?"""
        return voter_ids <= set(self.votes.keys())

    def resolve_votes(self, voter_ids):
        """Szavazás kiértékelése. Visszatér: 'vote_accepted' | 'vote_rejected'."""
        total_voters = len(voter_ids)
        if total_voters == 0:
            return 'vote_accepted'

        accept_count = sum(
            1 for vid in voter_ids
            if self.votes.get(vid) != 'reject'
        )

        if accept_count * 2 >= total_voters:
            return 'vote_accepted'
        return 'vote_rejected'

    def update_player_sid(self, old_id, new_id):
        """Játékos SID frissítése újracsatlakozáskor."""
        if old_id in self.votes:
            self.votes[new_id] = self.votes.pop(old_id)

    def to_state_dict(self, players):
        """Szerializálás a kliensnek."""
        placer = players[self.player_idx]
        return {
            'player_id': placer.id,
            'player_name': placer.name,
            'words': self.word_strs,
            'score': self.score,
            'tiles': [
                {'row': r, 'col': c, 'letter': l, 'is_blank': b}
                for r, c, l, b in self.tiles_placed
            ],
            'votes': dict(self.votes),
            'player_count': len(players),
            'expires_at': self.expires_at,
        }
