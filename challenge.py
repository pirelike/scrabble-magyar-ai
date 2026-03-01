class Challenge:
    """Challenge (megtámadás) állapot gép.

    Életciklus:
      1. Létrehozás: PENDING (megtámadási ablak)
      2. Szavazás indítása (3+ játékos): VOTING
      3. Lezárás: elfogadás vagy elutasítás
    """

    def __init__(self, tiles_placed, formed_words, word_strs, score,
                 player_idx, removed_from_hand):
        self.tiles_placed = tiles_placed
        self.formed_words = formed_words
        self.word_strs = word_strs
        self.score = score
        self.player_idx = player_idx
        self.removed_from_hand = removed_from_hand
        self.accepted_players = set()
        self.voting_phase = False
        self.challenger_id = None
        self.votes = {}

    def start_voting(self, challenger_id):
        """Szavazási fázis indítása. A korábbi elfogadásokat szavazatként átvezeti."""
        self.voting_phase = True
        self.challenger_id = challenger_id
        for pid in self.accepted_players:
            self.votes[pid] = 'accept'

    def add_accept(self, player_id):
        """Elfogadás rögzítése (megtámadási ablakban, 3+ játékos)."""
        self.accepted_players.add(player_id)

    def add_vote(self, player_id, vote):
        """Szavazat rögzítése a szavazási fázisban."""
        self.votes[player_id] = vote

    def all_accepted(self, non_placer_ids):
        """Mindenki elfogadta-e (megtámadási ablakban)?"""
        return self.accepted_players >= non_placer_ids

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
        if self.challenger_id == old_id:
            self.challenger_id = new_id
        if old_id in self.accepted_players:
            self.accepted_players.discard(old_id)
            self.accepted_players.add(new_id)
        if old_id in self.votes:
            self.votes[new_id] = self.votes.pop(old_id)

    def to_state_dict(self, players):
        """Szerializálás a kliensnek."""
        placer = players[self.player_idx]
        state = {
            'player_id': placer.id,
            'player_name': placer.name,
            'words': self.word_strs,
            'score': self.score,
            'tiles': [
                {'row': r, 'col': c, 'letter': l, 'is_blank': b}
                for r, c, l, b in self.tiles_placed
            ],
            'voting_phase': self.voting_phase,
            'votes': dict(self.votes),
            'accepted_players': list(self.accepted_players),
            'challenger_id': self.challenger_id,
            'player_count': len(players),
        }
        if self.challenger_id:
            for p in players:
                if p.id == self.challenger_id:
                    state['challenger_name'] = p.name
                    break
        return state
