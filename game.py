from tiles import TileBag, TILE_VALUES
from board import Board

HAND_SIZE = 7
BONUS_ALL_TILES = 50
CHALLENGE_TIMEOUT = 30  # másodperc


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


class Game:
    """Scrabble játék állapot és logika."""

    def __init__(self, game_id, challenge_mode=False):
        self.id = game_id
        self.players = []
        self.board = Board()
        self.bag = TileBag()
        self.current_player_idx = 0
        self.started = False
        self.finished = False
        self.winner = None
        self.turn_number = 0
        self.last_action = None
        self.challenge_mode = challenge_mode

        # Challenge állapot: ha van függő lerakás, ami megtámadható
        # None ha nincs, egyébként dict:
        # {tiles_placed, formed_words, score, player_idx, removed_from_hand}
        self.pending_challenge = None

    def add_player(self, player_id, name):
        if len(self.players) >= 4:
            return False, "Maximum 4 játékos lehet."
        if self.started:
            return False, "A játék már elkezdődött."
        player = Player(player_id, name)
        self.players.append(player)
        return True, "Csatlakozás sikeres."

    def mark_disconnected(self, player_id):
        """Jelöli a játékost ideiglenesen lecsatlakozottnak."""
        for p in self.players:
            if p.id == player_id:
                p.disconnected = True
                return True
        return False

    def replace_player_sid(self, old_id, new_id):
        """Kicseréli a játékos sid-jét újracsatlakozáskor."""
        for p in self.players:
            if p.id == old_id:
                p.id = new_id
                p.disconnected = False
                # pending_challenge-ben is frissítjük ha kell
                if self.pending_challenge:
                    pc = self.pending_challenge
                    if pc.get('challenger_id') == old_id:
                        pc['challenger_id'] = new_id
                    if old_id in pc.get('accepted_players', set()):
                        pc['accepted_players'].discard(old_id)
                        pc['accepted_players'].add(new_id)
                    if old_id in pc.get('votes', {}):
                        pc['votes'][new_id] = pc['votes'].pop(old_id)
                return True
        return False

    def remove_player(self, player_id):
        # Megkeressük az eltávolítandó játékos indexét
        removed_idx = None
        for i, p in enumerate(self.players):
            if p.id == player_id:
                removed_idx = i
                break
        if removed_idx is None:
            return

        # Ha a távozó játékos éppen challenge fázisban van, töröljük
        if self.pending_challenge and self.pending_challenge['player_idx'] == removed_idx:
            self.pending_challenge = None

        self.players.pop(removed_idx)

        # current_player_idx kiigazítása
        if self.players:
            if removed_idx < self.current_player_idx:
                # Az eltávolított játékos a jelenlegi előtt volt
                self.current_player_idx -= 1
            elif removed_idx == self.current_player_idx:
                # Az aktuális játékos lett eltávolítva
                # Az index maradhat, de ha túlmutat a lista végén, visszaállítjuk
                if self.current_player_idx >= len(self.players):
                    self.current_player_idx = 0
            # Ha removed_idx > current_player_idx, nem kell módosítani

            # pending_challenge player_idx kiigazítása
            if self.pending_challenge:
                pidx = self.pending_challenge['player_idx']
                if removed_idx < pidx:
                    self.pending_challenge['player_idx'] = pidx - 1
        else:
            self.current_player_idx = 0

    def start(self):
        if len(self.players) < 1:
            return False, "Legalább 1 játékos kell."
        if self.started:
            return False, "A játék már elkezdődött."

        self.started = True
        # Minden játékos húz 7 zsetont
        for player in self.players:
            player.hand = self.bag.draw(HAND_SIZE)
        return True, "A játék elkezdődött!"

    def current_player(self):
        if not self.players:
            return None
        return self.players[self.current_player_idx]

    def _next_turn(self):
        """Következő játékos. Kihagyja a büntetett játékosokat."""
        for _ in range(len(self.players)):
            self.current_player_idx = (self.current_player_idx + 1) % len(self.players)
            self.turn_number += 1
            current = self.players[self.current_player_idx]
            if current.skip_next_turn:
                current.skip_next_turn = False
                self.last_action = f"{current.name} kihagy egy kört (sikertelen megtámadás)"
            else:
                break

    def place_tiles(self, player_id, tiles_placed):
        """
        Betűk lerakása.
        tiles_placed: [(row, col, letter, is_blank), ...]
        Visszatér: (success, message, score)
        """
        if self.finished:
            return False, "A játék véget ért.", 0

        if self.pending_challenge:
            return False, "Várj a megtámadási fázis végéig.", 0

        player = self.current_player()
        if player.id != player_id:
            return False, "Nem te következel.", 0

        # Ellenőrizzük, hogy a játékosnak megvannak-e a betűi
        hand_copy = list(player.hand)
        for r, c, letter, is_blank in tiles_placed:
            if is_blank:
                if '' not in hand_copy:
                    return False, f"Nincs üres zsetonod.", 0
                hand_copy.remove('')
            else:
                if letter not in hand_copy:
                    return False, f"Nincs '{letter}' betűd.", 0
                hand_copy.remove(letter)

        # Tábla validáció - challenge módban szótár nélkül
        valid, formed_words, error = self.board.validate_placement(
            tiles_placed, skip_dictionary=self.challenge_mode
        )
        if not valid:
            return False, error, 0

        # Pontozás
        total_score = sum(score for _, _, score in formed_words)

        # 50 pont bónusz ha mind a 7 zsetont lerakta
        if len(tiles_placed) == HAND_SIZE:
            total_score += BONUS_ALL_TILES

        word_strs = [w for w, _, _ in formed_words]

        if self.challenge_mode and len(self.players) > 1:
            # Challenge mód: lerakás függőben, megtámadható/elfogadható
            # Eltávolítjuk a betűket a kézből
            removed = []
            for r, c, letter, is_blank in tiles_placed:
                if is_blank:
                    player.hand.remove('')
                    removed.append('')
                else:
                    player.hand.remove(letter)
                    removed.append(letter)

            self.pending_challenge = {
                'tiles_placed': tiles_placed,
                'formed_words': formed_words,
                'word_strs': word_strs,
                'score': total_score,
                'player_idx': self.current_player_idx,
                'removed_from_hand': removed,
                'accepted_players': set(),
                'voting_phase': False,
                'challenger_id': None,
                'votes': {},
            }

            self.last_action = f"{player.name}: {', '.join(word_strs)} ({total_score} pont) — megtámadható!"
            return True, f"Szavak: {', '.join(word_strs)} — megtámadható!", total_score

        # Normál mód (vagy egyjátékos challenge módban): azonnal véglegesít
        self.board.apply_placement(tiles_placed)
        player.score += total_score

        # Eltávolítjuk a felhasznált betűket a kézből
        for r, c, letter, is_blank in tiles_placed:
            if is_blank:
                player.hand.remove('')
            else:
                player.hand.remove(letter)

        # Új betűk húzása
        new_tiles = self.bag.draw(HAND_SIZE - len(player.hand))
        player.hand.extend(new_tiles)

        # Passz számláló reset
        player.consecutive_passes = 0

        self.last_action = f"{player.name}: {', '.join(word_strs)} ({total_score} pont)"

        # Játék vége ellenőrzés
        if len(player.hand) == 0 and self.bag.is_empty():
            self._end_game(player)
        else:
            self._next_turn()

        return True, f"Szavak: {', '.join(word_strs)}", total_score

    def challenge(self, challenger_id):
        """
        Megtámadás: szavazás indítása (csak 3+ játékos).
        2 játékos módban nincs megtámadás.
        Visszatér: (success, result, message)
          - result: 'voting' | 'vote_accepted' | 'vote_rejected' | None
        """
        if not self.pending_challenge:
            return False, None, "Nincs megtámadható lerakás."

        pending = self.pending_challenge
        placer = self.players[pending['player_idx']]

        if placer.id == challenger_id:
            return False, None, "Saját lerakásodat nem támadhatod meg."

        if len(self.players) <= 2:
            return False, None, "Két játékos módban nincs megtámadás."

        if pending.get('voting_phase'):
            return False, None, "Már folyamatban van a szavazás."

        challenger = None
        for p in self.players:
            if p.id == challenger_id:
                challenger = p
                break
        if not challenger:
            return False, None, "Nem vagy a játék résztvevője."

        # Szavazási fázis indítása
        pending['voting_phase'] = True
        pending['challenger_id'] = challenger_id

        # Korábbi elfogadások szavazatként átvezetése
        votes = {}
        for pid in pending.get('accepted_players', set()):
            votes[pid] = 'accept'
        pending['votes'] = votes

        # Ha már minden szavazó szavazott, azonnal kiértékeljük
        voter_ids = self._get_voter_ids()
        if voter_ids <= set(pending['votes'].keys()):
            result = self._resolve_votes()
            return True, result, self._make_vote_message(result)

        self.last_action = (
            f"{challenger.name} megtámadta {placer.name} szavait — szavazás!"
        )
        return True, 'voting', "Szavazás indult!"

    def cast_vote(self, player_id, vote):
        """
        Szavazat leadása a szavazási fázisban.
        vote: 'accept' | 'reject'
        Visszatér: (success, result, message)
        """
        if not self.pending_challenge:
            return False, None, "Nincs függő lerakás."

        pending = self.pending_challenge
        if not pending.get('voting_phase'):
            return False, None, "Nincs szavazás folyamatban."

        placer = self.players[pending['player_idx']]
        if placer.id == player_id:
            return False, None, "A lerakó nem szavazhat."

        if pending.get('challenger_id') == player_id:
            return False, None, "A megtámadó nem szavazhat."

        voter_ids = self._get_voter_ids()
        if player_id not in voter_ids:
            return False, None, "Nem szavazhatsz."

        if player_id in pending['votes']:
            return False, None, "Már szavaztál."

        voter = None
        for p in self.players:
            if p.id == player_id:
                voter = p
                break
        if not voter:
            return False, None, "Nem vagy a játék résztvevője."

        pending['votes'][player_id] = vote

        # Ha mindenki szavazott, kiértékelés
        if voter_ids <= set(pending['votes'].keys()):
            result = self._resolve_votes()
            return True, result, self._make_vote_message(result)

        self.last_action = f"{voter.name} szavazott."
        return True, 'vote_recorded', f"{voter.name} szavazott."

    def reject_pending_by_player(self, player_id):
        """
        Játékos elutasítja a függő lerakást (2 játékos mód).
        Visszatér: (success, result, message)
        """
        if not self.pending_challenge:
            return False, None, "Nincs függő lerakás."

        pending = self.pending_challenge
        placer = self.players[pending['player_idx']]

        if placer.id == player_id:
            return False, None, "Saját lerakásodat nem utasíthatod el."

        if len(self.players) > 2:
            return False, None, "3+ játékos módban használd a szavazást."

        if pending.get('voting_phase'):
            return False, None, "Szavazás folyamatban."

        self._finalize_reject()
        return True, 'rejected', "Lerakás elutasítva."

    def accept_pending_by_player(self, player_id):
        """
        Játékos elfogadja a függő lerakást.
        2 játékosnál azonnali elfogadás, 3+-nál elfogadás rögzítése vagy szavazat.
        Visszatér: (success, result, message)
        """
        if not self.pending_challenge:
            return False, None, "Nincs függő lerakás."

        pending = self.pending_challenge
        placer = self.players[pending['player_idx']]

        if placer.id == player_id:
            return False, None, "Saját lerakásodat nem fogadhatod el."

        # Szavazási fázisban: elfogadó szavazat
        if pending.get('voting_phase'):
            return self.cast_vote(player_id, 'accept')

        # 2 játékos: azonnali elfogadás
        if len(self.players) <= 2:
            self._finalize_accept()
            return True, 'accepted', "Lerakás elfogadva."

        # 3+ játékos: elfogadás rögzítése
        if player_id in pending.get('accepted_players', set()):
            return False, None, "Már elfogadtad."

        pending['accepted_players'].add(player_id)

        # Mindenki elfogadta?
        non_placer_ids = {p.id for p in self.players if p.id != placer.id}
        if pending['accepted_players'] >= non_placer_ids:
            self._finalize_accept()
            return True, 'accepted', "Lerakás elfogadva."

        return True, 'recorded', "Elfogadva, várakozás a többi játékosra."

    def accept_pending(self):
        """
        Függő lerakás elfogadása (timeout).
        Visszatér: (success, result, message)
        """
        if not self.pending_challenge:
            return False, None, "Nincs függő lerakás."

        pending = self.pending_challenge

        if pending.get('voting_phase'):
            # Szavazási idő lejárt: kiértékelés (nem szavazók = elfogadás)
            result = self._resolve_votes()
            if result == 'vote_accepted':
                return True, result, "Szavak elfogadva (szavazás lejárt)."
            else:
                return True, result, "Szavak elutasítva (szavazás lejárt)."
        else:
            # Megtámadási ablak lejárt: automatikus elfogadás
            self._finalize_accept()
            return True, 'accepted', "Lerakás elfogadva."

    def _get_voter_ids(self):
        """Szavazásra jogosult játékosok (nem lerakó, nem megtámadó)."""
        if not self.pending_challenge:
            return set()
        pending = self.pending_challenge
        placer_id = self.players[pending['player_idx']].id
        challenger_id = pending.get('challenger_id')
        return {
            p.id for p in self.players
            if p.id != placer_id and p.id != challenger_id
        }

    def _resolve_votes(self):
        """Szavazás kiértékelése. Visszatér: 'vote_accepted' | 'vote_rejected'."""
        pending = self.pending_challenge
        voter_ids = self._get_voter_ids()
        total_voters = len(voter_ids)

        if total_voters == 0:
            self._finalize_accept()
            return 'vote_accepted'

        # Szavazatok összesítése (nem szavazó = elfogadás)
        accept_count = 0
        for vid in voter_ids:
            vote = pending['votes'].get(vid)
            if vote != 'reject':
                accept_count += 1

        # 50% vagy több elfogadás → szó marad
        if accept_count * 2 >= total_voters:
            self._finalize_accept()
            return 'vote_accepted'
        else:
            self._finalize_reject()
            return 'vote_rejected'

    def _finalize_accept(self):
        """Lerakás véglegesítése (elfogadva)."""
        pending = self.pending_challenge
        player = self.players[pending['player_idx']]
        self.pending_challenge = None

        self.board.apply_placement(pending['tiles_placed'])
        player.score += pending['score']
        new_tiles = self.bag.draw(HAND_SIZE - len(player.hand))
        player.hand.extend(new_tiles)
        player.consecutive_passes = 0

        word_strs = pending['word_strs']
        self.last_action = f"{player.name}: {', '.join(word_strs)} ({pending['score']} pont)"

        if len(player.hand) == 0 and self.bag.is_empty():
            self._end_game(player)
        else:
            self._next_turn()

    def _finalize_reject(self):
        """Lerakás elutasítása. Betűk visszakerülnek, a lerakó újra jön."""
        pending = self.pending_challenge
        player = self.players[pending['player_idx']]
        self.pending_challenge = None

        player.hand.extend(pending['removed_from_hand'])
        player.consecutive_passes = 0

        word_strs = pending['word_strs']
        self.last_action = (
            f"{player.name} szavai elutasítva: "
            f"{', '.join(word_strs)}. Betűk visszavéve, újra ő következik."
        )
        # NEM hívunk _next_turn()-t: a lerakó újra próbálkozhat

    def _make_vote_message(self, result):
        """Szavazás eredmény üzenet."""
        if result == 'vote_accepted':
            return "A szavak elfogadva szavazással."
        else:
            return "A szavak elutasítva szavazással!"

    def exchange_tiles(self, player_id, tile_indices):
        """Betűcsere. tile_indices: a cserélendő betűk indexei a kézben."""
        if self.finished:
            return False, "A játék véget ért."

        if self.pending_challenge:
            return False, "Várj a megtámadási fázis végéig."

        player = self.current_player()
        if player.id != player_id:
            return False, "Nem te következel."

        if self.bag.remaining() < len(tile_indices):
            return False, "Nincs elég zseton a zsákban a cseréhez."

        if not tile_indices:
            return False, "Legalább egy zsetont ki kell választani."

        # Duplikált indexek ellenőrzése
        if len(tile_indices) != len(set(tile_indices)):
            return False, "Duplikált zseton index."

        # Ellenőrizzük az indexeket
        for idx in tile_indices:
            if idx < 0 or idx >= len(player.hand):
                return False, "Érvénytelen zseton index."

        # Kivesszük a cserélendő zsetonokat
        sorted_desc = sorted(tile_indices, reverse=True)
        tiles_to_exchange = [player.hand[i] for i in sorted_desc]
        for i in sorted_desc:
            player.hand.pop(i)

        # Új zsetonok húzása
        new_tiles = self.bag.draw(len(tiles_to_exchange))
        player.hand.extend(new_tiles)

        # Régi zsetonok visszarakása
        self.bag.put_back(tiles_to_exchange)

        player.consecutive_passes = 0
        self.last_action = f"{player.name} cserélt {len(tiles_to_exchange)} zsetont"
        self._next_turn()

        return True, f"{len(tiles_to_exchange)} zseton kicserélve."

    def pass_turn(self, player_id):
        """Passz."""
        if self.finished:
            return False, "A játék véget ért."

        if self.pending_challenge:
            return False, "Várj a megtámadási fázis végéig."

        player = self.current_player()
        if player.id != player_id:
            return False, "Nem te következel."

        player.consecutive_passes += 1
        self.last_action = f"{player.name} passzolt"

        # Ellenőrizzük, hogy mindenki 2x passzolt-e egymás után
        all_passed_twice = all(p.consecutive_passes >= 2 for p in self.players)
        if all_passed_twice:
            self._end_game(None)
        else:
            self._next_turn()

        return True, "Passz."

    def _end_game(self, finisher):
        """Játék vége, végső pontozás."""
        self.finished = True
        self.pending_challenge = None

        # Megmaradt betűk összpontszáma
        remaining_total = 0
        for player in self.players:
            hand_value = sum(TILE_VALUES.get(t, 0) for t in player.hand)
            player.score -= hand_value
            remaining_total += hand_value

        # Ha valaki az összes zsetonját felhasználta, megkapja a többiek pontjait
        if finisher:
            finisher.score += remaining_total

        # Győztes meghatározása
        self.winner = max(self.players, key=lambda p: p.score)
        self.last_action = f"Játék vége! Győztes: {self.winner.name} ({self.winner.score} pont)"

    def _get_shared_state(self):
        """Visszaadja a játék közös állapotát (ami minden játékosnál azonos).
        Cached: egyszer számítja ki, és get_state() újrahasználja."""
        current = self.current_player()
        state = {
            'game_id': self.id,
            'started': self.started,
            'finished': self.finished,
            'board': self.board.to_dict(),
            'current_player': current.id if current else None,
            'current_player_name': current.name if current else None,
            'turn_number': self.turn_number,
            'tiles_remaining': self.bag.remaining(),
            'last_action': self.last_action,
            'winner': self.winner.to_dict() if self.winner else None,
            'challenge_mode': self.challenge_mode,
            'pending_challenge': None,
        }

        if self.pending_challenge:
            pending = self.pending_challenge
            placer = self.players[pending['player_idx']]
            pc_state = {
                'player_id': placer.id,
                'player_name': placer.name,
                'words': pending['word_strs'],
                'score': pending['score'],
                'tiles': [
                    {'row': r, 'col': c, 'letter': l, 'is_blank': b}
                    for r, c, l, b in pending['tiles_placed']
                ],
                'voting_phase': pending.get('voting_phase', False),
                'votes': dict(pending.get('votes', {})),
                'accepted_players': list(pending.get('accepted_players', set())),
                'challenger_id': pending.get('challenger_id'),
                'player_count': len(self.players),
            }
            if pending.get('challenger_id'):
                for p in self.players:
                    if p.id == pending['challenger_id']:
                        pc_state['challenger_name'] = p.name
                        break
            state['pending_challenge'] = pc_state

        return state

    def get_state(self, for_player_id=None, _shared=None):
        """Visszaadja a játék állapotát JSON-kompatibilis formában.
        _shared: előre kiszámított közös állapot (get_all_states()-ből).
        """
        if _shared is None:
            _shared = self._get_shared_state()

        state = dict(_shared)
        state['players'] = [
            player.to_dict(reveal_hand=(player.id == for_player_id))
            for player in self.players
        ]
        return state

    def get_all_states(self):
        """Visszaadja az összes játékos állapotát egyszerre.
        A közös részt (board, challenge, stb.) csak egyszer számítja ki.
        Visszatér: {player_id: state_dict, ...}
        """
        shared = self._get_shared_state()
        return {
            player.id: self.get_state(for_player_id=player.id, _shared=shared)
            for player in self.players
        }
