mod common;

use common::reference;
use pg_scrabble::game::{Game, GameEnd, Turn};
use pg_scrabble::movegen::MoveGenerator;
use pg_scrabble::rng::Rng;
use pg_scrabble::rules::GameConfig;
use pg_scrabble::tile::{Tile, TILE_CODES};

fn census(game: &Game) -> [u8; TILE_CODES] {
    let mut counts = *game.bag().counts();
    for player in game.players() {
        for (code, n) in player.rack.counts().iter().enumerate() {
            counts[code] += n;
        }
    }
    for square in game.board().squares() {
        if square.is_occupied() {
            let tile = if square.is_blank() {
                Tile::BLANK
            } else {
                Tile::letter(square.index_unchecked())
            };
            counts[tile.code() as usize] += 1;
        }
    }
    counts
}

fn play_out(seed: u64) -> Game {
    let config = GameConfig::standard();
    let lexicon = reference::lexicon(&config);
    let mut generator = MoveGenerator::new(&config);
    let mut game = Game::new(config, &["north", "south"], seed);

    let mut turns = 0;
    while !game.is_over() && turns < 200 {
        let best = game
            .legal_plays(&mut generator, &lexicon)
            .iter()
            .max_by_key(|p| p.score())
            .copied();
        let turn = match best {
            Some(play) => Turn::Place(play),
            None if game.bag().len() >= game.config().min_exchange_bag => {
                Turn::Exchange(*game.current_rack())
            }
            None => Turn::Pass,
        };
        game.apply(turn, &lexicon)
            .expect("the chosen turn is legal");
        turns += 1;
    }
    assert!(game.is_over(), "a greedy game must terminate");
    game
}

#[test]
fn tiles_are_conserved_for_the_whole_game() {
    let config = GameConfig::standard();
    let start = *config.distribution.counts();
    for seed in 0..5 {
        let game = play_out(seed);
        assert_eq!(
            census(&game),
            start,
            "seed {seed} lost or invented tiles by the end"
        );
    }
}

#[test]
fn a_greedy_game_ends_by_playing_out_or_by_passing() {
    for seed in 0..5 {
        let game = play_out(seed);
        match game.end() {
            Some(GameEnd::PlayedOut { player }) => {
                assert!(
                    game.players()[player].rack.is_empty(),
                    "the player who went out should hold nothing"
                );
                assert!(game.bag().is_empty());
            }
            Some(GameEnd::ScorelessTurns) => {}
            None => panic!("play_out returned a running game"),
        }
    }
}

#[test]
fn scores_are_never_invented() {
    let config = GameConfig::standard();
    for seed in 0..5 {
        let game = play_out(seed);

        let from_turns: i32 = game.history().iter().map(|r| r.score).sum();
        let on_the_board: i32 = game.scores().iter().sum();
        assert!(
            on_the_board <= from_turns,
            "seed {seed}: scoreboard shows {on_the_board} from {from_turns} points of play"
        );
        assert!(
            game.history().iter().all(|r| r.score >= 0),
            "no turn may score negatively"
        );
        assert_eq!(
            config.distribution.total(),
            100,
            "sanity: the standard bag is unchanged"
        );
    }
}

#[test]
fn undo_restores_the_previous_position_exactly() {
    let config = GameConfig::standard();
    let lexicon = reference::lexicon(&config);
    let mut generator = MoveGenerator::new(&config);
    let mut rng = Rng::seed_from_u64(4242);
    let mut game = Game::new(config, &["north", "south"], 11);

    for _ in 0..30 {
        if game.is_over() {
            break;
        }
        let plays: Vec<_> = game.legal_plays(&mut generator, &lexicon).to_vec();
        let turn = if plays.is_empty() {
            Turn::Pass
        } else {
            Turn::Place(plays[rng.below(plays.len() as u64) as usize])
        };

        let before = game.clone();
        game.apply(turn, &lexicon)
            .expect("the chosen turn is legal");
        let after = game.clone();
        game.undo().expect("a turn was just played");

        assert_eq!(game.board(), before.board(), "undo left the board changed");
        assert_eq!(
            game.scores(),
            before.scores(),
            "undo left the scores changed"
        );
        assert_eq!(game.to_move(), before.to_move());
        assert_eq!(game.history().len(), before.history().len());
        assert_eq!(
            game.players().iter().map(|p| p.rack).collect::<Vec<_>>(),
            before.players().iter().map(|p| p.rack).collect::<Vec<_>>(),
            "undo left a rack changed"
        );
        assert_eq!(
            game.bag().remaining(),
            before.bag().remaining(),
            "undo left the bag changed"
        );

        game.apply(turn, &lexicon).expect("replaying a legal turn");
        assert_eq!(game.board(), after.board());
        assert_eq!(game.scores(), after.scores());
        assert_eq!(game.bag().remaining(), after.bag().remaining());
    }
}

#[test]
fn undo_after_an_exchange_conserves_tiles() {
    let config = GameConfig::standard();
    let lexicon = reference::lexicon(&config);
    let start = *config.distribution.counts();
    let mut game = Game::new(config, &["north", "south"], 5);

    let tiles = *game.current_rack();
    game.apply(Turn::Exchange(tiles), &lexicon).unwrap();
    assert_eq!(census(&game), start);

    game.undo().expect("an exchange was just played");
    assert_eq!(census(&game), start);
    assert_eq!(*game.current_rack(), tiles, "the original rack comes back");
    assert_eq!(game.to_move(), 0);
    assert!(game.history().is_empty());
}

#[test]
fn undo_walks_all_the_way_back_to_the_start() {
    let config = GameConfig::standard();
    let lexicon = reference::lexicon(&config);
    let mut generator = MoveGenerator::new(&config);
    let mut game = Game::new(config, &["north", "south"], 3);
    let start = game.clone();

    let mut played = 0;
    for _ in 0..12 {
        let best = game
            .legal_plays(&mut generator, &lexicon)
            .iter()
            .max_by_key(|p| p.score())
            .copied();
        let Some(play) = best else { break };
        game.apply(Turn::Place(play), &lexicon).unwrap();
        played += 1;
    }
    assert!(played > 3, "expected a few turns to unwind");

    while game.undo().is_some() {}
    assert_eq!(game.board(), start.board());
    assert_eq!(game.scores(), start.scores());
    assert_eq!(game.bag().remaining(), start.bag().remaining());
    assert_eq!(game.to_move(), start.to_move());
}

#[test]
fn a_finished_game_reports_a_winner_or_a_tie() {
    for seed in 0..5 {
        let game = play_out(seed);
        let scores = game.scores();
        match game.winner() {
            Some(w) => assert!(
                scores
                    .iter()
                    .enumerate()
                    .all(|(i, s)| i == w || *s < scores[w]),
                "the reported winner is not the sole top scorer"
            ),
            None => {
                let best = scores.iter().max().unwrap();
                assert!(
                    scores.iter().filter(|s| *s == best).count() > 1,
                    "a tie was reported without tied scores"
                );
            }
        }
    }
}
