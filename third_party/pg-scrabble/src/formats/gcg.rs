use crate::board::{Coord, Direction};
use crate::game::{Game, Turn};
use crate::rules::Alphabet;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::fmt;

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct GcgPlayer {
    pub nick: String,
    pub name: String,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum GcgEvent {
    Move {
        nick: String,
        rack: String,
        coord: Coord,
        dir: Direction,
        word: String,
        score: i32,
        cumulative: i32,
    },
    Exchange {
        nick: String,
        rack: String,
        tiles: String,
        cumulative: i32,
    },
    Pass {
        nick: String,
        rack: String,
        cumulative: i32,
    },
    Adjustment {
        nick: String,
        rack: String,
        score: i32,
        cumulative: i32,
    },
    Other(String),
}

#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct Gcg {
    pub players: Vec<GcgPlayer>,
    pub pragmata: Vec<String>,
    pub events: Vec<GcgEvent>,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum GcgError {
    Malformed { line: usize, text: String },
    BadCoord { line: usize, text: String },
    BadScore { line: usize, text: String },
}

impl fmt::Display for GcgError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            GcgError::Malformed { line, text } => {
                write!(f, "line {line} is not a valid turn: {text:?}")
            }
            GcgError::BadCoord { line, text } => {
                write!(f, "line {line} has an unreadable coordinate: {text:?}")
            }
            GcgError::BadScore { line, text } => {
                write!(f, "line {line} has an unreadable score: {text:?}")
            }
        }
    }
}

#[cfg(feature = "std")]
impl std::error::Error for GcgError {}

impl Gcg {
    pub fn parse(text: &str) -> Result<Gcg, GcgError> {
        let mut gcg = Gcg::default();

        for (n, raw) in text.lines().enumerate() {
            let line = raw.trim();
            let number = n + 1;
            if line.is_empty() {
                continue;
            }

            if let Some(rest) = line.strip_prefix('#') {
                if let Some(player) = parse_player_pragma(rest) {
                    gcg.players.push(player);
                } else {
                    gcg.pragmata.push(line.to_string());
                }
                continue;
            }

            if let Some(rest) = line.strip_prefix('>') {
                gcg.events.push(parse_event(rest, number)?);
                continue;
            }

            gcg.events.push(GcgEvent::Other(line.to_string()));
        }
        Ok(gcg)
    }

    pub fn to_text(&self) -> String {
        let mut out = String::new();
        for (i, player) in self.players.iter().enumerate() {
            out.push_str(&alloc::format!(
                "#player{} {} {}\n",
                i + 1,
                player.nick,
                player.name
            ));
        }
        for pragma in &self.pragmata {
            out.push_str(pragma);
            out.push('\n');
        }
        for event in &self.events {
            out.push_str(&format_event(event));
            out.push('\n');
        }
        out
    }

    pub fn from_game(game: &Game, alphabet: &Alphabet) -> Gcg {
        let players: Vec<GcgPlayer> = game
            .players()
            .iter()
            .map(|p| GcgPlayer {
                nick: p.name.replace(char::is_whitespace, "_"),
                name: p.name.clone(),
            })
            .collect();

        let mut running = alloc::vec![0i32; players.len()];
        let mut events = Vec::with_capacity(game.history().len());

        for record in game.history() {
            let nick = players[record.player].nick.clone();
            let rack = record.rack_before.to_text(alphabet);
            running[record.player] += record.score;
            let cumulative = running[record.player];

            events.push(match record.turn {
                Turn::Place(play) => GcgEvent::Move {
                    nick,
                    rack,
                    coord: play.coord(),
                    dir: play.direction(),
                    word: placement_text(&play, alphabet),
                    score: record.score,
                    cumulative,
                },
                Turn::Exchange(tiles) => GcgEvent::Exchange {
                    nick,
                    rack,
                    tiles: tiles.to_text(alphabet),
                    cumulative,
                },
                Turn::Pass => GcgEvent::Pass {
                    nick,
                    rack,
                    cumulative,
                },
            });
        }

        if game.is_over() {
            for (i, player) in game.players().iter().enumerate() {
                let adjustment = player.score - running[i];
                if adjustment != 0 {
                    events.push(GcgEvent::Adjustment {
                        nick: players[i].nick.clone(),
                        rack: player.rack.to_text(alphabet),
                        score: adjustment,
                        cumulative: player.score,
                    });
                }
            }
        }

        Gcg {
            players,
            pragmata: alloc::vec!["#character-encoding UTF-8".to_string()],
            events,
        }
    }
}

fn placement_text(play: &crate::movegen::Play, alphabet: &Alphabet) -> String {
    let placed: Vec<Coord> = play.placements().map(|(c, _)| c).collect();
    let (row, col) = (play.coord().row, play.coord().col);
    play.word()
        .iter()
        .enumerate()
        .map(|(i, square)| {
            let here = match play.direction() {
                Direction::Horizontal => Coord::new(row, col + i),
                Direction::Vertical => Coord::new(row + i, col),
            };
            if !placed.contains(&here) {
                return ".".to_string();
            }
            let s = alphabet.display(square.index_unchecked());
            if square.is_blank() {
                s.to_lowercase()
            } else {
                s.to_string()
            }
        })
        .collect()
}

fn parse_player_pragma(rest: &str) -> Option<GcgPlayer> {
    let rest = rest.strip_prefix("player")?;
    let rest = rest.trim_start_matches(|c: char| c.is_ascii_digit());
    let mut parts = rest.trim().splitn(2, char::is_whitespace);
    let nick = parts.next()?.trim();
    if nick.is_empty() {
        return None;
    }
    let name = parts.next().unwrap_or(nick).trim();
    Some(GcgPlayer {
        nick: nick.to_string(),
        name: name.to_string(),
    })
}

fn parse_event(rest: &str, line: usize) -> Result<GcgEvent, GcgError> {
    let malformed = || GcgError::Malformed {
        line,
        text: rest.to_string(),
    };
    let (nick, body) = rest.split_once(':').ok_or_else(malformed)?;
    let nick = nick.trim().to_string();
    let fields: Vec<&str> = body.split_whitespace().collect();

    let number = |s: &str| -> Result<i32, GcgError> {
        s.trim_start_matches('+')
            .parse::<i32>()
            .map_err(|_| GcgError::BadScore {
                line,
                text: s.to_string(),
            })
    };

    if let Some(first) = fields.first() {
        if first.starts_with('(') {
            if fields.len() < 3 {
                return Err(malformed());
            }
            let rack = first.trim_matches(|c| c == '(' || c == ')').to_string();
            return Ok(GcgEvent::Adjustment {
                nick,
                rack,
                score: number(fields[1])?,
                cumulative: number(fields[2])?,
            });
        }
    }

    match fields.as_slice() {
        [rack, action, _score, cumulative] if action.starts_with('-') => {
            let cumulative = number(cumulative)?;
            let tiles = action.trim_start_matches('-');
            Ok(if tiles.is_empty() {
                GcgEvent::Pass {
                    nick,
                    rack: rack.to_string(),
                    cumulative,
                }
            } else {
                GcgEvent::Exchange {
                    nick,
                    rack: rack.to_string(),
                    tiles: tiles.to_string(),
                    cumulative,
                }
            })
        }

        [rack, position, word, score, cumulative] => {
            let (coord, dir) = Coord::parse(position).ok_or(GcgError::BadCoord {
                line,
                text: position.to_string(),
            })?;
            Ok(GcgEvent::Move {
                nick,
                rack: rack.to_string(),
                coord,
                dir,
                word: word.to_string(),
                score: number(score)?,
                cumulative: number(cumulative)?,
            })
        }

        _ => Err(malformed()),
    }
}

fn format_event(event: &GcgEvent) -> String {
    match event {
        GcgEvent::Move {
            nick,
            rack,
            coord,
            dir,
            word,
            score,
            cumulative,
        } => alloc::format!(
            ">{nick}: {rack} {} {word} +{score} {cumulative}",
            coord.format(*dir)
        ),
        GcgEvent::Exchange {
            nick,
            rack,
            tiles,
            cumulative,
        } => alloc::format!(">{nick}: {rack} -{tiles} +0 {cumulative}"),
        GcgEvent::Pass {
            nick,
            rack,
            cumulative,
        } => alloc::format!(">{nick}: {rack} - +0 {cumulative}"),
        GcgEvent::Adjustment {
            nick,
            rack,
            score,
            cumulative,
        } => alloc::format!(">{nick}: ({rack}) {score:+} {cumulative}"),
        GcgEvent::Other(text) => text.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::board::Direction;

    const SAMPLE: &str = "\
#character-encoding UTF-8
#player1 gina Gina Wilson
#player2 raj Raj Patel
>gina: AEINRST 8D RETAINS +74 74
>raj: CDEIOUV -CDUV +0 0
>gina: BEGLOPY H8 .OPE +21 95
>raj: AEIOU - +0 0
>gina: (QZ) -20 75
";

    #[test]
    fn the_header_names_both_players() {
        let g = Gcg::parse(SAMPLE).unwrap();
        assert_eq!(g.players.len(), 2);
        assert_eq!(g.players[0].nick, "gina");
        assert_eq!(g.players[0].name, "Gina Wilson");
        assert_eq!(g.players[1].nick, "raj");
        assert_eq!(g.pragmata, ["#character-encoding UTF-8"]);
    }

    #[test]
    fn each_kind_of_turn_is_recognised() {
        let g = Gcg::parse(SAMPLE).unwrap();
        assert_eq!(g.events.len(), 5);

        match &g.events[0] {
            GcgEvent::Move {
                nick,
                rack,
                coord,
                dir,
                word,
                score,
                cumulative,
            } => {
                assert_eq!(nick, "gina");
                assert_eq!(rack, "AEINRST");
                assert_eq!(*coord, Coord::new(7, 3));
                assert_eq!(*dir, Direction::Horizontal);
                assert_eq!(word, "RETAINS");
                assert_eq!(*score, 74);
                assert_eq!(*cumulative, 74);
            }
            other => panic!("expected a move, got {other:?}"),
        }

        assert!(matches!(
            &g.events[1],
            GcgEvent::Exchange { tiles, .. } if tiles == "CDUV"
        ));
        assert!(matches!(
            &g.events[2],
            GcgEvent::Move { word, dir, .. }
                if word == ".OPE" && *dir == Direction::Vertical
        ));
        assert!(matches!(&g.events[3], GcgEvent::Pass { .. }));
        assert!(matches!(
            &g.events[4],
            GcgEvent::Adjustment {
                score: -20,
                cumulative: 75,
                ..
            }
        ));
    }

    #[test]
    fn text_round_trips() {
        let g = Gcg::parse(SAMPLE).unwrap();
        let text = g.to_text();
        let again = Gcg::parse(&text).unwrap();
        assert_eq!(g, again, "parsing our own output must give the same thing");
    }

    #[test]
    fn unknown_pragmata_survive_a_round_trip() {
        let text = "#note this is a comment\n#lexicon CSW21\n>a: AB - +0 0\n";
        let g = Gcg::parse(text).unwrap();
        assert_eq!(g.pragmata.len(), 2);
        assert!(g.to_text().contains("#lexicon CSW21"));
        assert!(g.to_text().contains("#note this is a comment"));
    }

    #[test]
    fn malformed_lines_are_reported_with_their_position() {
        let err = Gcg::parse(">gina: nonsense\n").unwrap_err();
        assert!(matches!(err, GcgError::Malformed { line: 1, .. }));

        let err = Gcg::parse(">gina: AB ZZ WORD +1 1\n").unwrap_err();
        assert!(matches!(err, GcgError::BadCoord { line: 1, .. }), "{err:?}");

        let err = Gcg::parse(">gina: AB 8D WORD +x 1\n").unwrap_err();
        assert!(matches!(err, GcgError::BadScore { line: 1, .. }));
    }

    #[test]
    fn blank_lines_are_ignored() {
        let g = Gcg::parse("\n\n#player1 a A\n\n>a: AB - +0 0\n\n").unwrap();
        assert_eq!(g.players.len(), 1);
        assert_eq!(g.events.len(), 1);
    }

    #[test]
    fn a_played_game_writes_a_transcript_that_parses_back() {
        use crate::game::Game;
        use crate::lexicon::Lexicon;
        use crate::movegen::MoveGenerator;
        use crate::rules::GameConfig;

        let config = GameConfig::standard();
        let alphabet = config.alphabet.clone();
        let lexicon = Lexicon::from_words(&alphabet, crate::lexicon_test_words()).unwrap();
        let mut generator = MoveGenerator::new(&config);
        let mut game = Game::new(config, &["gina", "raj"], 9);

        for _ in 0..8 {
            let best = game
                .legal_plays(&mut generator, &lexicon)
                .iter()
                .max_by_key(|p| p.score())
                .copied();
            let turn = match best {
                Some(p) => Turn::Place(p),
                None => Turn::Pass,
            };
            if game.apply(turn, &lexicon).is_err() {
                break;
            }
        }

        let gcg = Gcg::from_game(&game, &alphabet);
        assert_eq!(gcg.players.len(), 2);
        assert_eq!(gcg.events.len(), game.history().len());

        let text = gcg.to_text();
        let parsed = Gcg::parse(&text).expect("our own output must parse");
        assert_eq!(parsed, gcg);
    }

    #[test]
    fn played_through_tiles_are_written_as_dots() {
        use crate::board::Board;
        use crate::lexicon::Lexicon;
        use crate::movegen::MoveGenerator;
        use crate::rack::Rack;
        use crate::rules::GameConfig;
        use crate::tile::Square;

        let config = GameConfig::standard();
        let lexicon = Lexicon::from_words(&config.alphabet, crate::lexicon_test_words()).unwrap();
        let mut board = Board::new(&config.layout);
        for (i, ch) in "CAT".chars().enumerate() {
            let (l, _) = config.alphabet.parse_char(ch).unwrap();
            board.set(7, 7 + i, Square::letter(l));
        }

        let mut generator = MoveGenerator::new(&config);
        let rack = Rack::parse(&config.alphabet, "S").unwrap();
        let play = generator
            .generate(&board, &rack, &lexicon)
            .iter()
            .find(|p| p.word_text(&config.alphabet) == "CATS")
            .copied()
            .expect("CATS should be playable");

        assert_eq!(placement_text(&play, &config.alphabet), "...S");
    }
}
