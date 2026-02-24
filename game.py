from tiles import TileBag, TILE_VALUES
from board import Board

HAND_SIZE = 7
BONUS_ALL_TILES = 50


class Player:
    """Egy játékos állapota."""

    def __init__(self, player_id, name):
        self.id = player_id
        self.name = name
        self.hand = []  # Betűzsetonok a kézben
        self.score = 0
        self.consecutive_passes = 0

    def to_dict(self, reveal_hand=False):
        data = {
            'id': self.id,
            'name': self.name,
            'score': self.score,
            'hand_count': len(self.hand),
        }
        if reveal_hand:
            data['hand'] = self.hand
        return data


class Game:
    """Scrabble játék állapot és logika."""

    def __init__(self, game_id):
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

    def add_player(self, player_id, name):
        if len(self.players) >= 4:
            return False, "Maximum 4 játékos lehet."
        if self.started:
            return False, "A játék már elkezdődött."
        player = Player(player_id, name)
        self.players.append(player)
        return True, "Csatlakozás sikeres."

    def remove_player(self, player_id):
        # Megkeressük az eltávolítandó játékos indexét
        removed_idx = None
        for i, p in enumerate(self.players):
            if p.id == player_id:
                removed_idx = i
                break
        if removed_idx is None:
            return

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
        self.current_player_idx = (self.current_player_idx + 1) % len(self.players)
        self.turn_number += 1

    def place_tiles(self, player_id, tiles_placed):
        """
        Betűk lerakása.
        tiles_placed: [(row, col, letter, is_blank), ...]
        Visszatér: (success, message, score)
        """
        if self.finished:
            return False, "A játék véget ért.", 0

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

        # Tábla validáció
        valid, formed_words, error = self.board.validate_placement(tiles_placed)
        if not valid:
            return False, error, 0

        # Pontozás
        total_score = sum(score for _, _, score in formed_words)

        # 50 pont bónusz ha mind a 7 zsetont lerakta
        if len(tiles_placed) == HAND_SIZE:
            total_score += BONUS_ALL_TILES

        # Véglegesítés
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

        word_strs = [w for w, _, _ in formed_words]
        self.last_action = f"{player.name}: {', '.join(word_strs)} ({total_score} pont)"

        # Játék vége ellenőrzés
        if len(player.hand) == 0 and self.bag.is_empty():
            self._end_game(player)
        else:
            self._next_turn()

        return True, f"Szavak: {', '.join(word_strs)}", total_score

    def exchange_tiles(self, player_id, tile_indices):
        """Betűcsere. tile_indices: a cserélendő betűk indexei a kézben."""
        if self.finished:
            return False, "A játék véget ért."

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
        tiles_to_exchange = [player.hand[i] for i in sorted(tile_indices, reverse=True)]
        for i in sorted(tile_indices, reverse=True):
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

    def get_state(self, for_player_id=None):
        """Visszaadja a játék állapotát JSON-kompatibilis formában."""
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
            'players': [],
            'winner': self.winner.to_dict() if self.winner else None,
        }

        for player in self.players:
            reveal = (player.id == for_player_id)
            state['players'].append(player.to_dict(reveal_hand=reveal))

        return state
