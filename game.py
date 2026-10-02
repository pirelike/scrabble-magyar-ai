import json

from tiles import TileBag, TILE_VALUES
from board import Board, BOARD_SIZE
from challenge import Challenge
from player import Player  # noqa: F401 — re-export for backward compat

HAND_SIZE = 7
BONUS_ALL_TILES = 50
CHALLENGE_TIMEOUT = 30  # másodperc
# Ennyi egymást követő pont nélküli kör (passz, csere, elutasított lerakás) után véget ér a játék
SCORELESS_TURNS_LIMIT = 6
# Tippek száma játékonként (0 = kikapcsolva); a szoba létrehozásakor választható
ALLOWED_HINT_LIMITS = (0, 1, 3, 5, 10)
DEFAULT_HINT_LIMIT = 3


class Game:
    """Scrabble játék állapot és logika."""

    def __init__(self, game_id, challenge_mode=False, turn_time_limit=0,
                 hint_limit=DEFAULT_HINT_LIMIT):
        self.id = game_id
        self.players = []
        self.board = Board()
        self.bag = TileBag()
        self.current_player_idx = 0
        self.started = False
        self.finished = False
        self.winners = []  # döntetlennél több is lehet
        self.scoreless_turns = 0  # egymást követő pont nélküli körök száma
        self.turn_number = 0
        self.last_action = None
        self.challenge_mode = challenge_mode
        self.turn_time_limit = turn_time_limit  # 0 = kikapcsolt
        self.hint_limit = hint_limit if hint_limit in ALLOWED_HINT_LIMITS else DEFAULT_HINT_LIMIT
        self.hints_used = 0
        self.pending_challenge = None  # Challenge instance or None
        self.move_log = []  # Lépések listája
        self.last_action_info = None  # Az utolsó akció szerkezetes leírása (a kliens ezt fordítja)
        # A szavazással elutasított lerakások az aktuális táblaállásnál: a robot nem rakja le újra
        self.rejected_placements = set()
        self._history_cache = None  # (move_log hossza, történet)
        self._bot_seq = 0

    def add_player(self, player_id, name):
        if any(p.id == player_id for p in self.players):
            return False, "Ez a játékos már csatlakozott."
        if len(self.players) >= 4:
            return False, "Maximum 4 játékos lehet."
        if self.started:
            return False, "A játék már elkezdődött."
        player = Player(player_id, name)
        self.players.append(player)
        return True, "Csatlakozás sikeres."

    def add_bot(self, name, difficulty):
        """Számítógépes ellenfelet ad a játékhoz (csak indítás előtt)."""
        if len(self.players) >= 4:
            return False, "Maximum 4 játékos lehet."
        if self.started:
            return False, "A játék már elkezdődött."
        self._bot_seq += 1
        bot_id = f"bot-{self.id}-{self._bot_seq}"
        while any(p.id == bot_id for p in self.players):
            self._bot_seq += 1
            bot_id = f"bot-{self.id}-{self._bot_seq}"
        self.players.append(Player(bot_id, name, is_bot=True, difficulty=difficulty))
        return True, "Robot hozzáadva."

    @property
    def winner(self):
        """Az egyetlen győztes; döntetlennél (vagy játék közben) None."""
        return self.winners[0] if len(self.winners) == 1 else None

    @winner.setter
    def winner(self, player):
        self.winners = [player] if player else []

    def hints_left(self):
        """Hány tipp kérhető még (0, ha kikapcsolták vagy elfogytak)."""
        return max(0, self.hint_limit - self.hints_used)

    def use_hint(self):
        """Elhasznál egy tippet. Visszatér: True, ha volt még elérhető."""
        if self.hints_left() <= 0:
            return False
        self.hints_used += 1
        return True

    def human_players(self):
        """A nem robot játékosok."""
        return [p for p in self.players if not p.is_bot]

    def has_connected_human(self):
        """Van-e még kapcsolódott (nem lecsatlakozott) emberi játékos?"""
        return any(not p.is_bot and not p.disconnected for p in self.players)

    def _set_last_action(self, text, **info):
        """Az utolsó akció szövege + szerkezetes leírása (utóbbit a kliens lokalizálja)."""
        self.last_action = text
        self.last_action_info = dict(info) if info else None

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
        """Eltávolít egy játékost. Visszatér True-val, ha körtovábbítás szükséges."""
        removed_idx = None
        for i, p in enumerate(self.players):
            if p.id == player_id:
                removed_idx = i
                break
        if removed_idx is None:
            return False

        # Ha a távozó játékos éppen challenge fázisban van, töröljük
        if self.pending_challenge and self.pending_challenge.player_idx == removed_idx:
            self.pending_challenge = None

        was_current = (removed_idx == self.current_player_idx)
        self.players.pop(removed_idx)

        needs_next_turn = False
        if self.players:
            if removed_idx < self.current_player_idx:
                self.current_player_idx -= 1
            elif was_current:
                if self.current_player_idx >= len(self.players):
                    self.current_player_idx = 0
                if self.started and not self.finished:
                    needs_next_turn = True

            if self.pending_challenge:
                pidx = self.pending_challenge.player_idx
                if removed_idx < pidx:
                    self.pending_challenge.player_idx = pidx - 1
        else:
            self.current_player_idx = 0

        return needs_next_turn

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
        """Következő játékos. Kihagyja a büntetett és a lecsatlakozott játékosokat."""
        for _ in range(len(self.players)):
            self.current_player_idx = (self.current_player_idx + 1) % len(self.players)
            current = self.players[self.current_player_idx]

            # Ha le van csatlakozva, csak átlépjük (kivéve ha mindenki le van csatlakozva)
            if current.disconnected:
                # Csak akkor lépünk tovább, ha van még nem lecsatlakozott játékos
                if any(not p.disconnected for p in self.players):
                    continue
                else:
                    break

            if current.skip_next_turn:
                current.skip_next_turn = False
                self._set_last_action(f"{current.name} kihagy egy kört (sikertelen megtámadás)",
                                      type='skip', player=current.name)
            else:
                break
        self.turn_number += 1

    def skip_disconnected_current(self):
        """Ha a soron lévő játékos lecsatlakozott, tovább lépteti a kört.

        Függő megtámadás alatt nem lép: a lerakás lezárása (`_finalize_accept` /
        `_finalize_reject`) úgyis továbbadja a kört, itt a léptetés dupla ugrást okozna.
        Visszatér True-val, ha a kör tényleg továbblépett.
        """
        if not self.started or self.finished or self.pending_challenge:
            return False
        current = self.current_player()
        if not current or not current.disconnected:
            return False
        if all(p.disconnected for p in self.players):
            return False
        self._next_turn()
        return True

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
        rack = list(player.hand)  # a lépés előtti kéz (elemzéshez)
        self.board.apply_placement(tiles_placed)
        self.rejected_placements.clear()
        player.score += total_score
        self._remove_tiles_from_hand(player, tiles_placed)
        new_tiles = self.bag.draw(HAND_SIZE - len(player.hand))
        player.hand.extend(new_tiles)
        self.scoreless_turns = 0
        self._set_last_action(f"{player.name}: {', '.join(word_strs)} ({total_score} pont)",
                              type='place', player=player.name, words=list(word_strs),
                              score=total_score)
        self._record_move(player.name, 'place', tiles_placed=tiles_placed,
                          formed_words=formed_words, score=total_score, rack=rack)

        if len(player.hand) == 0 and self.bag.is_empty():
            self._end_game(player)
        else:
            self._next_turn()

    def _challenge_applies(self, placer):
        """Él-e a megtámadási (szavazásos) mód a lerakónál? Csak akkor, ha van legalább egy
        másik emberi játékos, aki szavazhat — a robotok nem szavaznak, velük szemben a
        szótár dönt."""
        if not self.challenge_mode:
            return False
        return any(p is not placer and not p.is_bot for p in self.players)

    def preview_placement(self, player_id, tiles_placed):
        """A lerakás kipróbálása véglegesítés nélkül (élő pontszám-előnézet).

        Visszatér: {'valid': bool, 'score': int, 'words': [{'word', 'score'}], 'message': str}
        """
        empty = {'valid': False, 'score': 0, 'words': [], 'message': ''}
        if self.finished or not self.started or self.pending_challenge:
            return empty
        player = self.current_player()
        if not player or player.id != player_id:
            return empty
        ok, err = self._validate_hand(player, tiles_placed)
        if not ok:
            return {**empty, 'message': err}
        valid, formed_words, error = self.board.validate_placement(
            tiles_placed, skip_dictionary=self._challenge_applies(player)
        )
        if not valid:
            return {**empty, 'message': error}
        total = sum(score for _, _, score in formed_words)
        if len(tiles_placed) == HAND_SIZE:
            total += BONUS_ALL_TILES
        return {
            'valid': True,
            'score': total,
            'words': [{'word': w, 'score': sc} for w, _, sc in formed_words],
            'message': '',
        }

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

        challenge_active = self._challenge_applies(player)
        valid, formed_words, error = self.board.validate_placement(
            tiles_placed, skip_dictionary=challenge_active
        )
        if not valid:
            return False, error, 0

        total_score = sum(score for _, _, score in formed_words)
        if len(tiles_placed) == HAND_SIZE:
            total_score += BONUS_ALL_TILES

        word_strs = [w for w, _, _ in formed_words]

        if challenge_active:
            removed = self._remove_tiles_from_hand(player, tiles_placed)
            self.pending_challenge = Challenge(
                tiles_placed=tiles_placed,
                formed_words=formed_words,
                word_strs=word_strs,
                score=total_score,
                player_idx=self.current_player_idx,
                removed_from_hand=removed,
            )
            self._set_last_action(
                f"{player.name}: {', '.join(word_strs)} ({total_score} pont) — szavazásra vár",
                type='pending', player=player.name, words=list(word_strs), score=total_score)
            return True, f"Szavak: {', '.join(word_strs)} — szavazásra vár!", total_score

        # Normál mód (vagy egyjátékos challenge módban): azonnal véglegesít
        self._finalize_placement(player, tiles_placed, total_score, word_strs,
                                 formed_words=formed_words)
        return True, f"Szavak: {', '.join(word_strs)}", total_score

    # --- Challenge system (voting) ---

    def _get_voter_ids(self):
        """Szavazásra jogosult játékosok (a lerakón kívül minden emberi játékos)."""
        if not self.pending_challenge:
            return set()
        pc = self.pending_challenge
        placer_id = self.players[pc.player_idx].id
        return {p.id for p in self.players if p.id != placer_id and not p.is_bot}

    def _finalize_accept(self):
        """Lerakás véglegesítése (elfogadva)."""
        pc = self.pending_challenge
        player = self.players[pc.player_idx]
        self.pending_challenge = None

        rack = list(player.hand) + list(pc.removed_from_hand)  # a lépés előtti kéz (elemzéshez)
        self.board.apply_placement(pc.tiles_placed)
        self.rejected_placements.clear()
        player.score += pc.score
        new_tiles = self.bag.draw(HAND_SIZE - len(player.hand))
        player.hand.extend(new_tiles)
        self.scoreless_turns = 0

        self._set_last_action(f"{player.name}: {', '.join(pc.word_strs)} ({pc.score} pont)",
                              type='place', player=player.name, words=list(pc.word_strs),
                              score=pc.score)
        self._record_move(player.name, 'challenge_accept', tiles_placed=pc.tiles_placed,
                          formed_words=pc.formed_words, score=pc.score, rack=rack)

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
        self.rejected_placements.add(frozenset(pc.tiles_placed))

        self._set_last_action(
            f"{player.name} szavai elutasítva: "
            f"{', '.join(pc.word_strs)}. Betűk visszavéve, újra ő következik.",
            type='rejected', player=player.name, words=list(pc.word_strs))
        self._record_move(player.name, 'challenge_reject')

        # Az elutasított lerakás pont nélküli körnek számít (különben végtelen próbálkozás lenne)
        if self._register_scoreless_turn():
            return
        if player.disconnected:
            # A lecsatlakozott lerakó nem tud újra lépni, ne akadjon el a játék.
            self._next_turn()

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

    def accept_pending_by_player(self, player_id):
        """Játékos elfogadja a függő lerakást (elfogadó szavazat).
        Visszatér: (success, result, message)
        """
        if not self.pending_challenge:
            return False, None, "Nincs függő lerakás."

        pc = self.pending_challenge
        placer = self.players[pc.player_idx]

        if placer.id == player_id:
            return False, None, "Saját lerakásodat nem fogadhatod el."

        voter_ids = self._get_voter_ids()
        if player_id not in voter_ids:
            return False, None, "Nem szavazhatsz."
        if player_id in pc.votes:
            return False, None, "Már szavaztál."

        voter = self._find_player(player_id)
        if not voter:
            return False, None, "Nem vagy a játék résztvevője."

        pc.add_vote(player_id, 'accept')

        if pc.all_voted(voter_ids):
            result = self._resolve_and_finalize()
            return True, result, self._make_vote_message(result)

        self._set_last_action(f"{voter.name} elfogadta.", type='vote', player=voter.name,
                              vote='accept')
        return True, 'vote_recorded', f"{voter.name} elfogadta."

    def reject_pending_by_player(self, player_id):
        """Játékos elutasítja a függő lerakást (elutasító szavazat).
        Visszatér: (success, result, message)
        """
        if not self.pending_challenge:
            return False, None, "Nincs függő lerakás."

        pc = self.pending_challenge
        placer = self.players[pc.player_idx]

        if placer.id == player_id:
            return False, None, "Saját lerakásodat nem utasíthatod el."

        voter_ids = self._get_voter_ids()
        if player_id not in voter_ids:
            return False, None, "Nem szavazhatsz."
        if player_id in pc.votes:
            return False, None, "Már szavaztál."

        voter = self._find_player(player_id)
        if not voter:
            return False, None, "Nem vagy a játék résztvevője."

        pc.add_vote(player_id, 'reject')

        if pc.all_voted(voter_ids):
            result = self._resolve_and_finalize()
            return True, result, self._make_vote_message(result)

        self._set_last_action(f"{voter.name} elutasította.", type='vote', player=voter.name,
                              vote='reject')
        return True, 'vote_recorded', f"{voter.name} elutasította."

    def withdraw_pending(self, player_id):
        """A lerakó visszavonja a még el nem döntött (szavazásra váró) lerakását.

        Csak addig lehet, amíg senki sem szavazott; a betűk visszakerülnek a kezébe, és újra ő
        következik. Nem számít körnek (a pont nélküli körök számlálója sem változik).
        Visszatér: (success, message)
        """
        if not self.pending_challenge:
            return False, "Nincs függő lerakás."
        pc = self.pending_challenge
        player = self.players[pc.player_idx]
        if player.id != player_id:
            return False, "Csak a lerakó vonhatja vissza a lerakását."
        if pc.votes:
            return False, "Már szavaztak, a lerakás már nem vonható vissza."

        self.pending_challenge = None
        player.hand.extend(pc.removed_from_hand)
        self._set_last_action(f"{player.name} visszavonta a lerakását.",
                              type='withdrawn', player=player.name, words=list(pc.word_strs))
        return True, "Lerakás visszavonva."

    def accept_pending(self):
        """Függő lerakás elfogadása (timeout).
        Visszatér: (success, result, message)
        """
        if not self.pending_challenge:
            return False, None, "Nincs függő lerakás."

        result = self._resolve_and_finalize()
        msg = ("Szavak elfogadva (időtúllépés)." if result == 'vote_accepted'
               else "Szavak elutasítva (időtúllépés).")
        return True, result, msg

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
        if self.bag.remaining() < 7:
            return False, "Nincs elég zseton a zsákban a cseréhez (minimum 7 kell)."
        if self.bag.remaining() < len(tile_indices):
            return False, "Nincs elég zseton a zsákban a cseréhez."
        if not tile_indices:
            return False, "Legalább egy zsetont ki kell választani."
        if len(tile_indices) != len(set(tile_indices)):
            return False, "Duplikált zseton index."

        for idx in tile_indices:
            if idx < 0 or idx >= len(player.hand):
                return False, "Érvénytelen zseton index."

        rack = list(player.hand)  # a csere előtti kéz (elemzéshez)
        sorted_desc = sorted(tile_indices, reverse=True)
        tiles_to_exchange = [player.hand[i] for i in sorted_desc]
        for i in sorted_desc:
            player.hand.pop(i)

        new_tiles = self.bag.draw(len(tiles_to_exchange))
        player.hand.extend(new_tiles)
        self.bag.put_back(tiles_to_exchange)

        self._set_last_action(f"{player.name} cserélt {len(tiles_to_exchange)} zsetont",
                              type='exchange', player=player.name, count=len(tiles_to_exchange))
        self._record_move(player.name, 'exchange', rack=rack)
        if not self._register_scoreless_turn():
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

        self._set_last_action(f"{player.name} passzolt", type='pass', player=player.name)
        self._record_move(player.name, 'pass', rack=list(player.hand))
        if not self._register_scoreless_turn():
            self._next_turn()

        return True, "Passz."

    # --- Helpers ---

    def _register_scoreless_turn(self):
        """Pont nélküli kör (passz, csere, elutasított lerakás) rögzítése.
        Visszatér True-val, ha ezzel véget ért a játék."""
        self.scoreless_turns += 1
        if self.scoreless_turns >= SCORELESS_TURNS_LIMIT:
            self._end_game(None)
            return True
        return False

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

        # Egyenlő pontnál mindenki nyer (döntetlen), nem a lista első játékosa
        top_score = max(p.score for p in self.players)
        self.winners = [p for p in self.players if p.score == top_score]
        if len(self.winners) == 1:
            winner = self.winners[0]
            self._set_last_action(f"Játék vége! Győztes: {winner.name} ({top_score} pont)",
                                  type='game_over', player=winner.name, score=top_score)
        else:
            names = [p.name for p in self.winners]
            self._set_last_action(f"Játék vége! Döntetlen: {', '.join(names)} ({top_score} pont)",
                                  type='game_over_draw', players=names, score=top_score)

    # --- Move logging & persistence ---

    def _board_snapshot(self):
        """Aktuális tábla állapot JSON-ként."""
        return self.board.to_dict()

    def _record_move(self, player_name, action_type, tiles_placed=None,
                     formed_words=None, score=0, rack=None):
        """Lépés rögzítése a move_log-ba. `rack`: a játékos keze a lépés előtt ('' = joker)."""
        details = {}
        if rack is not None:
            details['rack'] = list(rack)
        if tiles_placed:
            details['tiles'] = [
                {'row': r, 'col': c, 'letter': l, 'is_blank': b}
                for r, c, l, b in tiles_placed
            ]
        if formed_words:
            details['words'] = [w for w, _, _ in formed_words]
        if score is not None:
            details['score'] = score

        self.move_log.append({
            'move_number': len(self.move_log) + 1,
            'player_name': player_name,
            'action_type': action_type,
            'details_json': json.dumps(details, ensure_ascii=False),
            'board_snapshot_json': json.dumps(self._board_snapshot()),
        })

    def to_save_dict(self):
        """Teljes játékállapot szerializálása mentéshez.

        Függő (szavazásra váró) lerakás nem menthető: a lerakó betűit ilyenkor
        visszaírjuk a kezébe, így a mentésből betöltve nem vesznek el zsetonok.
        """
        pending_idx = None
        returned_tiles = []
        last_action = self.last_action
        last_action_info = self.last_action_info
        if self.pending_challenge:
            pending_idx = self.pending_challenge.player_idx
            returned_tiles = list(self.pending_challenge.removed_from_hand)
            placer = self.players[pending_idx]
            last_action = f"{placer.name} lerakása a mentés miatt visszavonva."
            last_action_info = {'type': 'save_revert', 'player': placer.name}

        return {
            'id': self.id,
            'challenge_mode': self.challenge_mode,
            'turn_time_limit': self.turn_time_limit,
            'hint_limit': self.hint_limit,
            'hints_used': self.hints_used,
            'started': self.started,
            'finished': self.finished,
            'current_player_idx': self.current_player_idx,
            'turn_number': self.turn_number,
            'scoreless_turns': self.scoreless_turns,
            'last_action': last_action,
            'last_action_info': last_action_info,
            'board': self.board.to_dict(),
            'board_is_empty': self.board.is_empty,
            'bag_tiles': list(self.bag.tiles),
            'players': [
                {
                    'id': p.id,
                    'name': p.name,
                    'hand': list(p.hand) + (returned_tiles if i == pending_idx else []),
                    'score': p.score,
                    'skip_next_turn': p.skip_next_turn,
                    'disconnected': p.disconnected,
                    'is_bot': p.is_bot,
                    'difficulty': p.difficulty,
                }
                for i, p in enumerate(self.players)
            ],
            'winner_names': [p.name for p in self.winners],
        }

    @classmethod
    def from_save_dict(cls, data):
        """Játék visszaállítása mentett állapotból."""
        game = cls(data['id'], challenge_mode=data.get('challenge_mode', False),
                   turn_time_limit=data.get('turn_time_limit', 0),
                   hint_limit=data.get('hint_limit', DEFAULT_HINT_LIMIT))
        game.hints_used = max(0, int(data.get('hints_used', 0) or 0))
        game.started = data.get('started', False)
        game.finished = data.get('finished', False)
        game.current_player_idx = data.get('current_player_idx', 0)
        game.turn_number = data.get('turn_number', 0)
        # Régi mentésben nincs játékszintű számláló: a játékosonkénti passz-sorozat legnagyobbja
        legacy_passes = max((pd.get('consecutive_passes', 0) for pd in data.get('players', [])),
                            default=0)
        game.scoreless_turns = max(0, int(data.get('scoreless_turns', legacy_passes) or 0))
        game.last_action = data.get('last_action')
        game.last_action_info = data.get('last_action_info')

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
            player = Player(pd['id'], pd['name'], is_bot=bool(pd.get('is_bot')),
                            difficulty=pd.get('difficulty'))
            player.hand = list(pd.get('hand', []))
            player.score = pd.get('score', 0)
            player.skip_next_turn = pd.get('skip_next_turn', False)
            player.disconnected = False if player.is_bot else pd.get('disconnected', False)
            game.players.append(player)

        # Győztes(ek) visszaállítása (régi mentésben egyetlen `winner_name`)
        winner_names = data.get('winner_names')
        if winner_names is None:
            winner_names = [data['winner_name']] if data.get('winner_name') else []
        game.winners = [p for p in game.players if p.name in winner_names]

        return game

    # --- Lépéstörténet ---

    def get_history(self):
        """A lépések rövid, kliensnek szánt listája (a move_log-ból, hosszra gyorsítótárazva)."""
        count = len(self.move_log)
        if self._history_cache and self._history_cache[0] == count:
            return self._history_cache[1]
        history = []
        for move in self.move_log:
            try:
                details = json.loads(move.get('details_json') or '{}')
            except (TypeError, ValueError):
                details = {}
            history.append({
                'n': move['move_number'],
                'player': move['player_name'],
                'type': move['action_type'],
                'words': details.get('words', []),
                'score': details.get('score', 0),
                'tiles': [{'row': t['row'], 'col': t['col']} for t in details.get('tiles', [])],
            })
        self._history_cache = (count, history)
        return history

    def _last_move_tiles(self):
        """Az utolsó lépés lerakott mezői (kiemeléshez), ha az utolsó lépés lerakás volt."""
        history = self.get_history()
        if not history or history[-1]['type'] not in ('place', 'challenge_accept'):
            return []
        return history[-1]['tiles']

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
            'last_action_info': self.last_action_info,
            'last_move_tiles': self._last_move_tiles(),
            'history': [{k: v for k, v in h.items() if k != 'tiles'} for h in self.get_history()],
            'winner': self.winner.to_dict() if self.winner else None,
            'winners': [p.to_dict() for p in self.winners],
            'challenge_mode': self.challenge_mode,
            'turn_time_limit': self.turn_time_limit,
            'hint_limit': self.hint_limit,
            'hints_left': self.hints_left(),
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
            if not player.is_bot
        }

    def get_spectator_state(self, _shared=None):
        """Megfigyelői nézet: ugyanaz, mint a játékosoké, de egyetlen kéz sem látszik."""
        state = self.get_state(for_player_id=None, _shared=_shared)
        state['spectator'] = True
        return state
