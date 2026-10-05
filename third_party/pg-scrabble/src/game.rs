use crate::bag::Bag;
use crate::board::Board;
use crate::lexicon::Lexicon;
use crate::movegen::{self, MoveGenerator, Play, PlayError};
use crate::rack::Rack;
use crate::rules::GameConfig;
use crate::tile::Tile;
use alloc::string::String;
use alloc::vec::Vec;
use core::fmt;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Turn {
    Place(Play),
    Exchange(Rack),
    Pass,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Player {
    pub name: String,
    pub rack: Rack,
    pub score: i32,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GameEnd {
    PlayedOut { player: usize },
    ScorelessTurns,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum TurnError {
    GameOver,
    Illegal(PlayError),
    RackMissingTiles,
    BagTooSmall { need: usize, have: usize },
    NothingToExchange,
}

impl fmt::Display for TurnError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TurnError::GameOver => write!(f, "the game is over"),
            TurnError::Illegal(e) => write!(f, "{e}"),
            TurnError::RackMissingTiles => write!(f, "the rack does not hold those tiles"),
            TurnError::BagTooSmall { need, have } => write!(
                f,
                "an exchange needs {need} tiles left in the bag, but {have} remain"
            ),
            TurnError::NothingToExchange => write!(f, "an exchange must include a tile"),
        }
    }
}

#[cfg(feature = "std")]
impl std::error::Error for TurnError {}

impl From<PlayError> for TurnError {
    fn from(e: PlayError) -> Self {
        TurnError::Illegal(e)
    }
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Record {
    pub player: usize,
    pub turn: Turn,
    pub score: i32,
    pub rack_before: Rack,
    pub drawn: Vec<Tile>,
    zeros_before: u32,
    scores_before: Vec<i32>,
    ended_before: Option<GameEnd>,
    adjusted_before: bool,
    to_move_before: usize,
}

#[derive(Clone, Debug)]
pub struct Game {
    config: GameConfig,
    board: Board,
    bag: Bag,
    players: Vec<Player>,
    to_move: usize,
    history: Vec<Record>,
    consecutive_zeros: u32,
    ended: Option<GameEnd>,
    adjusted: bool,
}

impl Game {
    pub fn new(config: GameConfig, names: &[&str], seed: u64) -> Game {
        assert!(names.len() >= 2, "a game needs at least two players");
        let mut bag = Bag::new(&config.distribution, seed);
        let board = Board::new(&config.layout);
        let players = names
            .iter()
            .map(|&name| {
                let mut rack = Rack::new();
                bag.refill(&mut rack, config.rack_size);
                Player {
                    name: name.into(),
                    rack,
                    score: 0,
                }
            })
            .collect();
        Game {
            config,
            board,
            bag,
            players,
            to_move: 0,
            history: Vec::new(),
            consecutive_zeros: 0,
            ended: None,
            adjusted: false,
        }
    }

    pub fn from_position(
        config: GameConfig,
        board: Board,
        players: Vec<Player>,
        to_move: usize,
        seed: u64,
    ) -> Game {
        let mut bag = Bag::new(&config.distribution, seed);
        for square in board.squares() {
            if square.is_occupied() {
                let tile = if square.is_blank() {
                    Tile::BLANK
                } else {
                    Tile::letter(square.index_unchecked())
                };
                bag.take_specific(tile);
            }
        }
        for player in &players {
            for tile in player.rack.tiles() {
                bag.take_specific(tile);
            }
        }
        Game {
            config,
            board,
            bag,
            players,
            to_move,
            history: Vec::new(),
            consecutive_zeros: 0,
            ended: None,
            adjusted: false,
        }
    }

    #[inline]
    pub fn config(&self) -> &GameConfig {
        &self.config
    }

    #[inline]
    pub fn board(&self) -> &Board {
        &self.board
    }

    #[inline]
    pub fn bag(&self) -> &Bag {
        &self.bag
    }

    #[inline]
    pub fn players(&self) -> &[Player] {
        &self.players
    }

    #[inline]
    pub fn to_move(&self) -> usize {
        self.to_move
    }

    #[inline]
    pub fn current(&self) -> &Player {
        &self.players[self.to_move]
    }

    #[inline]
    pub fn current_rack(&self) -> &Rack {
        &self.players[self.to_move].rack
    }

    #[inline]
    pub fn history(&self) -> &[Record] {
        &self.history
    }

    #[inline]
    pub fn end(&self) -> Option<GameEnd> {
        self.ended
    }

    #[inline]
    pub fn is_over(&self) -> bool {
        self.ended.is_some()
    }

    #[inline]
    pub fn consecutive_zeros(&self) -> u32 {
        self.consecutive_zeros
    }

    pub fn legal_plays<'g>(
        &self,
        generator: &'g mut MoveGenerator,
        lexicon: &Lexicon,
    ) -> &'g [Play] {
        generator.generate(&self.board, self.current_rack(), lexicon)
    }

    pub fn apply(&mut self, turn: Turn, lexicon: &Lexicon) -> Result<i32, TurnError> {
        if self.ended.is_some() {
            return Err(TurnError::GameOver);
        }
        let who = self.to_move;
        let rack_before = self.players[who].rack;
        let zeros_before = self.consecutive_zeros;
        let scores_before: Vec<i32> = self.players.iter().map(|p| p.score).collect();

        let (score, drawn) = match turn {
            Turn::Pass => {
                self.consecutive_zeros += 1;
                (0, Vec::new())
            }

            Turn::Exchange(tiles) => {
                if tiles.is_empty() {
                    return Err(TurnError::NothingToExchange);
                }
                if self.bag.len() < self.config.min_exchange_bag {
                    return Err(TurnError::BagTooSmall {
                        need: self.config.min_exchange_bag,
                        have: self.bag.len(),
                    });
                }
                if !self.players[who].rack.remove_all(&tiles) {
                    return Err(TurnError::RackMissingTiles);
                }

                let drawn = self.bag.draw_n(tiles.len());
                for t in &drawn {
                    self.players[who].rack.add(*t);
                }
                self.bag.put_back(tiles.tiles());
                self.consecutive_zeros += 1;
                (0, drawn)
            }

            Turn::Place(play) => {
                let score = movegen::validate(
                    &self.board,
                    &self.config,
                    lexicon,
                    &play,
                    Some(&self.players[who].rack),
                )?;
                play.apply(&mut self.board);
                let used = play.tiles();
                if !self.players[who].rack.remove_all(&used) {
                    play.undo(&mut self.board);
                    return Err(TurnError::RackMissingTiles);
                }
                let mut rack = self.players[who].rack;
                let drawn = self.bag.refill(&mut rack, self.config.rack_size);
                self.players[who].rack = rack;

                if score > 0 {
                    self.consecutive_zeros = 0;
                } else {
                    self.consecutive_zeros += 1;
                }
                (score, drawn)
            }
        };

        self.players[who].score += score;
        self.history.push(Record {
            player: who,
            turn,
            score,
            rack_before,
            drawn,
            zeros_before,
            scores_before,
            ended_before: self.ended,
            adjusted_before: self.adjusted,
            to_move_before: who,
        });

        if self.players[who].rack.is_empty() && self.bag.is_empty() {
            self.ended = Some(GameEnd::PlayedOut { player: who });
        } else if self.consecutive_zeros >= self.config.max_consecutive_zeros {
            self.ended = Some(GameEnd::ScorelessTurns);
        }
        if self.ended.is_some() {
            self.apply_end_adjustments();
        } else {
            self.to_move = (self.to_move + 1) % self.players.len();
        }
        Ok(score)
    }

    pub fn undo(&mut self) -> Option<Record> {
        let record = self.history.pop()?;
        let who = record.player;

        match &record.turn {
            Turn::Pass => {}
            Turn::Exchange(tiles) => {
                for t in tiles.tiles() {
                    self.bag.take_specific(t);
                }
                self.bag.return_exact(record.drawn.iter().rev().copied());
            }
            Turn::Place(play) => {
                play.undo(&mut self.board);
                self.bag.return_exact(record.drawn.iter().rev().copied());
            }
        }

        self.players[who].rack = record.rack_before;
        for (player, score) in self.players.iter_mut().zip(record.scores_before.iter()) {
            player.score = *score;
        }
        self.consecutive_zeros = record.zeros_before;
        self.ended = record.ended_before;
        self.adjusted = record.adjusted_before;
        self.to_move = record.to_move_before;
        Some(record)
    }

    fn apply_end_adjustments(&mut self) {
        if self.adjusted {
            return;
        }
        self.adjusted = true;
        let alphabet = &self.config.alphabet;
        let values: Vec<i32> = self
            .players
            .iter()
            .map(|p| p.rack.value(alphabet))
            .collect();

        for (i, player) in self.players.iter_mut().enumerate() {
            player.score -= values[i];
        }
        if self.config.double_out_adjustment {
            if let Some(GameEnd::PlayedOut { player }) = self.ended {
                let others: i32 = values
                    .iter()
                    .enumerate()
                    .filter(|(i, _)| *i != player)
                    .map(|(_, v)| *v)
                    .sum();
                self.players[player].score += others;
            }
        }
    }

    pub fn scores(&self) -> Vec<i32> {
        self.players.iter().map(|p| p.score).collect()
    }

    pub fn winner(&self) -> Option<usize> {
        let best = self.players.iter().map(|p| p.score).max()?;
        let mut winners = self
            .players
            .iter()
            .enumerate()
            .filter(|(_, p)| p.score == best);
        let first = winners.next()?;
        if winners.next().is_some() {
            None
        } else {
            Some(first.0)
        }
    }

    pub fn resign(&mut self) {
        if self.ended.is_none() {
            self.ended = Some(GameEnd::ScorelessTurns);
            self.apply_end_adjustments();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::Alphabet;

    fn lexicon() -> Lexicon {
        Lexicon::from_words(
            &Alphabet::english(),
            [
                "AT", "ATE", "CAT", "CATS", "EAT", "EATS", "SAT", "SEAT", "SEATS", "TA", "TAE",
                "STONE", "TONE", "ONE", "NOTE", "NOTES",
            ],
        )
        .unwrap()
    }

    fn game() -> Game {
        Game::new(GameConfig::standard(), &["A", "B"], 7)
    }

    #[test]
    fn a_new_game_deals_full_racks() {
        let g = game();
        assert_eq!(g.players().len(), 2);
        for p in g.players() {
            assert_eq!(p.rack.len(), 7);
            assert_eq!(p.score, 0);
        }
        assert_eq!(g.bag().len(), 100 - 14);
        assert_eq!(g.to_move(), 0);
        assert!(!g.is_over());
    }

    #[test]
    fn passing_advances_the_turn_and_counts_towards_the_end() {
        let lex = lexicon();
        let mut g = game();
        assert_eq!(g.apply(Turn::Pass, &lex), Ok(0));
        assert_eq!(g.to_move(), 1);
        assert_eq!(g.consecutive_zeros(), 1);
        assert_eq!(g.history().len(), 1);
    }

    #[test]
    fn six_scoreless_turns_end_the_game() {
        let lex = lexicon();
        let mut g = game();
        for _ in 0..5 {
            g.apply(Turn::Pass, &lex).unwrap();
            assert!(!g.is_over());
        }
        g.apply(Turn::Pass, &lex).unwrap();
        assert_eq!(g.end(), Some(GameEnd::ScorelessTurns));
        assert_eq!(g.apply(Turn::Pass, &lex), Err(TurnError::GameOver));
    }

    #[test]
    fn a_legal_play_scores_and_refills() {
        let lex = lexicon();
        let mut g = Game::from_position(
            GameConfig::standard(),
            Board::new(&crate::rules::BoardLayout::standard()),
            alloc::vec![
                Player {
                    name: "A".into(),
                    rack: Rack::parse(&Alphabet::english(), "CATSXYZ").unwrap(),
                    score: 0,
                },
                Player {
                    name: "B".into(),
                    rack: Rack::parse(&Alphabet::english(), "AEIOURT").unwrap(),
                    score: 0,
                },
            ],
            0,
            1,
        );
        let mut gen = MoveGenerator::new(g.config());
        let play = *g
            .legal_plays(&mut gen, &lex)
            .iter()
            .max_by_key(|p| p.score())
            .expect("an opening rack has plays");
        let expected = play.score();
        let scored = g.apply(Turn::Place(play), &lex).unwrap();
        assert_eq!(scored, expected);
        assert_eq!(g.players()[0].score, expected);
        assert_eq!(g.players()[0].rack.len(), 7, "the rack refills");
        assert_eq!(g.to_move(), 1);
        assert_eq!(g.consecutive_zeros(), 0);
        assert!(!g.board().is_empty());
    }

    #[test]
    fn an_illegal_play_is_rejected_and_changes_nothing() {
        let lex = lexicon();
        let mut g = game();
        let mut gen = MoveGenerator::new(g.config());

        let board = Board::new(&g.config().layout);
        let rack = Rack::parse(&Alphabet::english(), "CATS").unwrap();
        let play = *gen
            .generate(&board, &rack, &lex)
            .iter()
            .next()
            .expect("CATS can open");

        g.players[0].rack = Rack::parse(&Alphabet::english(), "QQQQQQQ").unwrap();
        let before = g.board().clone();
        let err = g.apply(Turn::Place(play), &lex).unwrap_err();
        assert!(matches!(
            err,
            TurnError::Illegal(PlayError::RackMissingTiles) | TurnError::RackMissingTiles
        ));
        assert_eq!(
            g.board(),
            &before,
            "a rejected play must not touch the board"
        );
        assert_eq!(g.to_move(), 0, "a rejected play does not pass the turn");
        assert!(g.history().is_empty());
    }

    #[test]
    fn exchanges_need_enough_tiles_in_the_bag() {
        let lex = lexicon();
        let mut g = game();
        let tiles = Rack::from_tiles(g.current_rack().tiles().into_iter().take(3));
        assert_eq!(g.apply(Turn::Exchange(tiles), &lex), Ok(0));
        assert_eq!(g.current_rack().len(), 7);
        assert_eq!(
            g.bag().len(),
            100 - 14,
            "an exchange returns as many as it takes"
        );

        while g.bag().len() >= g.config().min_exchange_bag {
            g.bag.draw();
        }
        g.to_move = 0;
        let tiles = Rack::from_tiles(g.current_rack().tiles().into_iter().take(1));
        assert!(matches!(
            g.apply(Turn::Exchange(tiles), &lex),
            Err(TurnError::BagTooSmall { .. })
        ));
    }

    #[test]
    fn exchanging_tiles_you_do_not_hold_is_rejected() {
        let lex = lexicon();
        let mut g = game();
        g.players[0].rack = Rack::parse(&Alphabet::english(), "AAAAAAA").unwrap();
        let tiles = Rack::parse(&Alphabet::english(), "ZZ").unwrap();
        assert_eq!(
            g.apply(Turn::Exchange(tiles), &lex),
            Err(TurnError::RackMissingTiles)
        );
        assert_eq!(
            g.current_rack().len(),
            7,
            "a rejected exchange changes nothing"
        );
    }

    #[test]
    fn going_out_transfers_the_opponents_tiles() {
        let lex = lexicon();
        let alphabet = Alphabet::english();
        let mut config = GameConfig::standard();
        config.rack_size = 3;

        let layout = config.layout.clone();
        let mut g = Game::from_position(
            config,
            Board::new(&layout),
            alloc::vec![
                Player {
                    name: "A".into(),
                    rack: Rack::parse(&alphabet, "CAT").unwrap(),
                    score: 0,
                },
                Player {
                    name: "B".into(),
                    rack: Rack::parse(&alphabet, "QZ").unwrap(),
                    score: 0,
                },
            ],
            0,
            3,
        );

        while g.bag.draw().is_some() {}

        let mut gen = MoveGenerator::new(g.config());
        let play = *g
            .legal_plays(&mut gen, &lex)
            .iter()
            .find(|p| p.tiles_used() == 3)
            .expect("CAT uses the whole rack");
        let scored = g.apply(Turn::Place(play), &lex).unwrap();

        assert_eq!(g.end(), Some(GameEnd::PlayedOut { player: 0 }));

        assert_eq!(g.players()[0].score, scored + 20);
        assert_eq!(g.players()[1].score, -20);
        assert_eq!(g.winner(), Some(0));
    }

    #[test]
    fn a_scoreless_ending_only_subtracts() {
        let lex = lexicon();
        let alphabet = Alphabet::english();
        let config = GameConfig::standard();
        let layout = config.layout.clone();
        let mut g = Game::from_position(
            config,
            Board::new(&layout),
            alloc::vec![
                Player {
                    name: "A".into(),
                    rack: Rack::parse(&alphabet, "QQ").unwrap(),
                    score: 100,
                },
                Player {
                    name: "B".into(),
                    rack: Rack::parse(&alphabet, "AA").unwrap(),
                    score: 100,
                },
            ],
            0,
            3,
        );
        for _ in 0..6 {
            g.apply(Turn::Pass, &lex).unwrap();
        }
        assert_eq!(g.end(), Some(GameEnd::ScorelessTurns));
        assert_eq!(g.players()[0].score, 100 - 20);
        assert_eq!(g.players()[1].score, 100 - 2);
        assert_eq!(g.winner(), Some(1));
    }

    #[test]
    fn adjustments_are_applied_exactly_once() {
        let lex = lexicon();
        let mut g = game();
        for _ in 0..6 {
            g.apply(Turn::Pass, &lex).unwrap();
        }
        let after = g.scores();
        g.resign();
        assert_eq!(
            g.scores(),
            after,
            "resigning a finished game changes nothing"
        );
    }
}
