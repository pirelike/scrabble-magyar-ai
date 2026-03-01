import json

from tiles import TileBag, TILE_VALUES
from board import Board, BOARD_SIZE
from challenge import Challenge

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
        self.pending_challenge = None  # Challenge instance or None
        self.move_log = []  # Lépések listája
        self._db_game_id = None  # DB ID a mentés után
        self._last_saved_move_count = 0  # Utolsó mentéskor hány lépés volt

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
                if self.pending_challenge:
                    self.pending_challenge.update_player_sid(old_id, new_id)
                return True
        return False

    def remove_player(self, player_id):
        removed_idx = None
        for i, p in enumerate(self.players):
            if p.id == player_id:
                removed_idx = i
                break
        if removed_idx is None:
            return

        # Ha a távozó játékos éppen challenge fázisban van, töröljük
        if self.pending_challenge and self.pending_challenge.player_idx == removed_idx:
            self.pending_challenge = None

        self.players.pop(removed_idx)

        if self.players:
            if removed_idx < self.current_player_idx:
                self.current_player_idx -= 1
            elif removed_idx == self.current_player_idx:
                if self.current_player_idx >= len(self.players):
                    self.current_player_idx = 0

            if self.pending_challenge:
                pidx = self.pending_challenge.player_idx
                if removed_idx < pidx:
                    self.pending_challenge.player_idx = pidx - 1
        else:
            self.current_player_idx = 0

    def start(self):
        if len(self.players) < 1:
            return False, "Legalább 1 játékos kell."
        if self.started:
            return False, "A játék már elkezdődött."

        self.started = True
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

    # --- Tile placement ---

    def _validate_hand(self, player, tiles_placed):
        """Ellenőrzi, hogy a játékosnak megvannak-e a lerakandó betűk.
        Visszatér: (ok, error_message)"""
        hand_copy = list(player.hand)
        for _r, _c, letter, is_blank in tiles_placed:
            target = '' if is_blank else letter
            if target not in hand_copy:
                if is_blank:
                    return False, "Nincs üres zsetonod."
                return False, f"Nincs '{letter}' betűd."
            hand_copy.remove(target)
        return True, ""

    def _remove_tiles_from_hand(self, player, tiles_placed):
        """Eltávolítja a lerakott betűket a kézből. Visszaadja az eltávolított betűket."""
        removed = []
        for _r, _c, letter, is_blank in tiles_placed:
            target = '' if is_blank else letter
            player.hand.remove(target)
            removed.append(target)
        return removed

    def _finalize_placement(self, player, tiles_placed, total_score, word_strs,
                            formed_words=None):
        """Véglegesíti a lerakást: tábla, pont, húzás, kör."""
        self.board.apply_placement(tiles_placed)
        player.score += total_score
        self._remove_tiles_from_hand(player, tiles_placed)
        new_tiles = self.bag.draw(HAND_SIZE - len(player.hand))
        player.hand.extend(new_tiles)
        player.consecutive_passes = 0
        self.last_action = f"{player.name}: {', '.join(word_strs)} ({total_score} pont)"
        self._record_move(player.name, 'place', tiles_placed=tiles_placed,
                          formed_words=formed_words, score=total_score)

        if len(player.hand) == 0 and self.bag.is_empty():
            self._end_game(player)
        else:
            self._next_turn()

    def place_tiles(self, player_id, tiles_placed):
        """Betűk lerakása.
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

        ok, err = self._validate_hand(player, tiles_placed)
        if not ok:
            return False, err, 0

        valid, formed_words, error = self.board.validate_placement(
            tiles_placed, skip_dictionary=self.challenge_mode
        )
        if not valid:
            return False, error, 0

        total_score = sum(score for _, _, score in formed_words)
        if len(tiles_placed) == HAND_SIZE:
            total_score += BONUS_ALL_TILES

        word_strs = [w for w, _, _ in formed_words]

        if self.challenge_mode and len(self.players) > 1:
            removed = self._remove_tiles_from_hand(player, tiles_placed)
            self.pending_challenge = Challenge(
                tiles_placed=tiles_placed,
                formed_words=formed_words,
                word_strs=word_strs,
                score=total_score,
                player_idx=self.current_player_idx,
                removed_from_hand=removed,
            )
            self.last_action = f"{player.name}: {', '.join(word_strs)} ({total_score} pont) — megtámadható!"
            return True, f"Szavak: {', '.join(word_strs)} — megtámadható!", total_score

        # Normál mód (vagy egyjátékos challenge módban): azonnal véglegesít
        self._finalize_placement(player, tiles_placed, total_score, word_strs,
                                 formed_words=formed_words)
        return True, f"Szavak: {', '.join(word_strs)}", total_score

    # --- Challenge system ---

    def _get_voter_ids(self):
        """Szavazásra jogosult játékosok (nem lerakó, nem megtámadó)."""
        if not self.pending_challenge:
            return set()
        pc = self.pending_challenge
        placer_id = self.players[pc.player_idx].id
        return {
            p.id for p in self.players
            if p.id != placer_id and p.id != pc.challenger_id
        }

    def _finalize_accept(self):
        """Lerakás véglegesítése (elfogadva)."""
        pc = self.pending_challenge
        player = self.players[pc.player_idx]
        self.pending_challenge = None

        self.board.apply_placement(pc.tiles_placed)
        player.score += pc.score
        new_tiles = self.bag.draw(HAND_SIZE - len(player.hand))
        player.hand.extend(new_tiles)
        player.consecutive_passes = 0

        self.last_action = f"{player.name}: {', '.join(pc.word_strs)} ({pc.score} pont)"
        self._record_move(player.name, 'challenge_accept', tiles_placed=pc.tiles_placed,
                          formed_words=pc.formed_words, score=pc.score)

        if len(player.hand) == 0 and self.bag.is_empty():
            self._end_game(player)
        else:
            self._next_turn()

    def _finalize_reject(self):
        """Lerakás elutasítása. Betűk visszakerülnek, a lerakó újra jön."""
        pc = self.pending_challenge
        player = self.players[pc.player_idx]
        self.pending_challenge = None

        player.hand.extend(pc.removed_from_hand)
        player.consecutive_passes = 0

        self.last_action = (
            f"{player.name} szavai elutasítva: "
            f"{', '.join(pc.word_strs)}. Betűk visszavéve, újra ő következik."
        )
        self._record_move(player.name, 'challenge_reject')

    def _resolve_and_finalize(self):
        """Szavazás kiértékelése és véglegesítése. Visszatér: result string."""
        voter_ids = self._get_voter_ids()
        result = self.pending_challenge.resolve_votes(voter_ids)
        if result == 'vote_accepted':
            self._finalize_accept()
        else:
            self._finalize_reject()
        return result

    @staticmethod
    def _make_vote_message(result):
        if result == 'vote_accepted':
            return "A szavak elfogadva szavazással."
        return "A szavak elutasítva szavazással!"

    def challenge(self, challenger_id):
        """Megtámadás: szavazás indítása (csak 3+ játékos).
        Visszatér: (success, result, message)
        """
        if not self.pending_challenge:
            return False, None, "Nincs megtámadható lerakás."

        pc = self.pending_challenge
        placer = self.players[pc.player_idx]

        if placer.id == challenger_id:
            return False, None, "Saját lerakásodat nem támadhatod meg."
        if len(self.players) <= 2:
            return False, None, "Két játékos módban nincs megtámadás."
        if pc.voting_phase:
            return False, None, "Már folyamatban van a szavazás."

        challenger = self._find_player(challenger_id)
        if not challenger:
            return False, None, "Nem vagy a játék résztvevője."

        pc.start_voting(challenger_id)

        # Ha már minden szavazó szavazott, azonnal kiértékeljük
        voter_ids = self._get_voter_ids()
        if pc.all_voted(voter_ids):
            result = self._resolve_and_finalize()
            return True, result, self._make_vote_message(result)

        self.last_action = (
            f"{challenger.name} megtámadta {placer.name} szavait — szavazás!"
        )
        return True, 'voting', "Szavazás indult!"

    def cast_vote(self, player_id, vote):
        """Szavazat leadása a szavazási fázisban.
        Visszatér: (success, result, message)
        """
        if not self.pending_challenge:
            return False, None, "Nincs függő lerakás."

        pc = self.pending_challenge
        if not pc.voting_phase:
            return False, None, "Nincs szavazás folyamatban."

        placer = self.players[pc.player_idx]
        if placer.id == player_id:
            return False, None, "A lerakó nem szavazhat."
        if pc.challenger_id == player_id:
            return False, None, "A megtámadó nem szavazhat."

        voter_ids = self._get_voter_ids()
        if player_id not in voter_ids:
            return False, None, "Nem szavazhatsz."
        if player_id in pc.votes:
            return False, None, "Már szavaztál."

        voter = self._find_player(player_id)
        if not voter:
            return False, None, "Nem vagy a játék résztvevője."

        pc.add_vote(player_id, vote)

        if pc.all_voted(voter_ids):
            result = self._resolve_and_finalize()
            return True, result, self._make_vote_message(result)

        self.last_action = f"{voter.name} szavazott."
        return True, 'vote_recorded', f"{voter.name} szavazott."

    def reject_pending_by_player(self, player_id):
        """Játékos elutasítja a függő lerakást (2 játékos mód).
        Visszatér: (success, result, message)
        """
        if not self.pending_challenge:
            return False, None, "Nincs függő lerakás."

        pc = self.pending_challenge
        placer = self.players[pc.player_idx]

        if placer.id == player_id:
            return False, None, "Saját lerakásodat nem utasíthatod el."
        if len(self.players) > 2:
            return False, None, "3+ játékos módban használd a szavazást."
        if pc.voting_phase:
            return False, None, "Szavazás folyamatban."

        self._finalize_reject()
        return True, 'rejected', "Lerakás elutasítva."

    def accept_pending_by_player(self, player_id):
        """Játékos elfogadja a függő lerakást.
        2 játékosnál azonnali elfogadás, 3+-nál elfogadás rögzítése vagy szavazat.
        Visszatér: (success, result, message)
        """
        if not self.pending_challenge:
            return False, None, "Nincs függő lerakás."

        pc = self.pending_challenge
        placer = self.players[pc.player_idx]

        if placer.id == player_id:
            return False, None, "Saját lerakásodat nem fogadhatod el."

        # Szavazási fázisban: elfogadó szavazat
        if pc.voting_phase:
            return self.cast_vote(player_id, 'accept')

        # 2 játékos: azonnali elfogadás
        if len(self.players) <= 2:
            self._finalize_accept()
            return True, 'accepted', "Lerakás elfogadva."

        # 3+ játékos: elfogadás rögzítése
        if player_id in pc.accepted_players:
            return False, None, "Már elfogadtad."

        pc.add_accept(player_id)

        non_placer_ids = {p.id for p in self.players if p.id != placer.id}
        if pc.all_accepted(non_placer_ids):
            self._finalize_accept()
            return True, 'accepted', "Lerakás elfogadva."

        return True, 'recorded', "Elfogadva, várakozás a többi játékosra."

    def accept_pending(self):
        """Függő lerakás elfogadása (timeout).
        Visszatér: (success, result, message)
        """
        if not self.pending_challenge:
            return False, None, "Nincs függő lerakás."

        if self.pending_challenge.voting_phase:
            result = self._resolve_and_finalize()
            msg = ("Szavak elfogadva (szavazás lejárt)." if result == 'vote_accepted'
                   else "Szavak elutasítva (szavazás lejárt).")
            return True, result, msg

        self._finalize_accept()
        return True, 'accepted', "Lerakás elfogadva."

    # --- Other actions ---

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
        if len(tile_indices) != len(set(tile_indices)):
            return False, "Duplikált zseton index."

        for idx in tile_indices:
            if idx < 0 or idx >= len(player.hand):
                return False, "Érvénytelen zseton index."

        sorted_desc = sorted(tile_indices, reverse=True)
        tiles_to_exchange = [player.hand[i] for i in sorted_desc]
        for i in sorted_desc:
            player.hand.pop(i)

        new_tiles = self.bag.draw(len(tiles_to_exchange))
        player.hand.extend(new_tiles)
        self.bag.put_back(tiles_to_exchange)

        player.consecutive_passes = 0
        self.last_action = f"{player.name} cserélt {len(tiles_to_exchange)} zsetont"
        self._record_move(player.name, 'exchange')
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
        self._record_move(player.name, 'pass')

        if all(p.consecutive_passes >= 2 for p in self.players):
            self._end_game(None)
        else:
            self._next_turn()

        return True, "Passz."

    # --- Helpers ---

    def _find_player(self, player_id):
        """Játékos keresése ID alapján."""
        for p in self.players:
            if p.id == player_id:
                return p
        return None

    def _end_game(self, finisher):
        """Játék vége, végső pontozás."""
        self.finished = True
        self.pending_challenge = None

        remaining_total = 0
        for player in self.players:
            hand_value = sum(TILE_VALUES.get(t, 0) for t in player.hand)
            player.score -= hand_value
            remaining_total += hand_value

        if finisher:
            finisher.score += remaining_total

        self.winner = max(self.players, key=lambda p: p.score)
        self.last_action = f"Játék vége! Győztes: {self.winner.name} ({self.winner.score} pont)"

    # --- Move logging & persistence ---

    def _board_snapshot(self):
        """Aktuális tábla állapot JSON-ként."""
        return self.board.to_dict()

    def _record_move(self, player_name, action_type, tiles_placed=None,
                     formed_words=None, score=0):
        """Lépés rögzítése a move_log-ba."""
        details = {}
        if tiles_placed:
            details['tiles'] = [
                {'row': r, 'col': c, 'letter': l, 'is_blank': b}
                for r, c, l, b in tiles_placed
            ]
        if formed_words:
            details['words'] = [w for w, _, _ in formed_words]
        if score:
            details['score'] = score

        self.move_log.append({
            'move_number': len(self.move_log) + 1,
            'player_name': player_name,
            'action_type': action_type,
            'details_json': json.dumps(details, ensure_ascii=False),
            'board_snapshot_json': json.dumps(self._board_snapshot()),
        })

    def to_save_dict(self):
        """Teljes játékállapot szerializálása mentéshez."""
        return {
            'id': self.id,
            'challenge_mode': self.challenge_mode,
            'started': self.started,
            'finished': self.finished,
            'current_player_idx': self.current_player_idx,
            'turn_number': self.turn_number,
            'last_action': self.last_action,
            'board': self.board.to_dict(),
            'board_is_empty': self.board.is_empty,
            'bag_tiles': list(self.bag.tiles),
            'players': [
                {
                    'id': p.id,
                    'name': p.name,
                    'hand': list(p.hand),
                    'score': p.score,
                    'consecutive_passes': p.consecutive_passes,
                    'skip_next_turn': p.skip_next_turn,
                }
                for p in self.players
            ],
            'winner_name': self.winner.name if self.winner else None,
        }

    @classmethod
    def from_save_dict(cls, data):
        """Játék visszaállítása mentett állapotból."""
        game = cls(data['id'], challenge_mode=data.get('challenge_mode', False))
        game.started = data.get('started', False)
        game.finished = data.get('finished', False)
        game.current_player_idx = data.get('current_player_idx', 0)
        game.turn_number = data.get('turn_number', 0)
        game.last_action = data.get('last_action')

        # Board visszaállítás
        board_data = data.get('board', [])
        for r in range(BOARD_SIZE):
            for c in range(BOARD_SIZE):
                cell = board_data[r][c] if r < len(board_data) and c < len(board_data[r]) else None
                if cell is not None:
                    game.board.cells[r][c] = (cell['letter'], cell['is_blank'])
        game.board.is_empty = data.get('board_is_empty', True)

        # Bag visszaállítás
        game.bag.tiles = list(data.get('bag_tiles', []))

        # Játékosok visszaállítás
        for pd in data.get('players', []):
            player = Player(pd['id'], pd['name'])
            player.hand = list(pd.get('hand', []))
            player.score = pd.get('score', 0)
            player.consecutive_passes = pd.get('consecutive_passes', 0)
            player.skip_next_turn = pd.get('skip_next_turn', False)
            game.players.append(player)

        # Winner visszaállítás
        winner_name = data.get('winner_name')
        if winner_name:
            for p in game.players:
                if p.name == winner_name:
                    game.winner = p
                    break

        return game

    # --- State serialization ---

    def _get_shared_state(self):
        """Visszaadja a játék közös állapotát (ami minden játékosnál azonos)."""
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
            state['pending_challenge'] = self.pending_challenge.to_state_dict(self.players)

        return state

    def get_state(self, for_player_id=None, _shared=None):
        """Visszaadja a játék állapotát JSON-kompatibilis formában."""
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
        A közös részt csak egyszer számítja ki.
        """
        shared = self._get_shared_state()
        return {
            player.id: self.get_state(for_player_id=player.id, _shared=shared)
            for player in self.players
        }
