//! A motor szabályai és szótára: ugyanaz a szókincs, mint a roboté, minden jogos zsetonbontásban.

use crate::ai::{BONUS_ALL_TILES, HAND_SIZE, Vocabulary};
use crate::game::SCORELESS_TURNS_LIMIT;
use crate::tiles::{TILE_DISTRIBUTION, Tile, forms_digraph};
use pg_scrabble::prelude::*;

/// A motor szabálykészlete a magyar játékhoz: 38 betű (a kétjegyűek egy zsetonok), 2 joker, 100 zseton, ugyanaz a
/// tábla, 7 zsetonos kéz, 50 pontos bingó, csere legalább 7 zseton a zsákban, 6 pont nélküli kör a vége.
///
/// A motor betűindexe a mi zsetonindexünk − 1 (nálunk a 0 a joker).
pub fn engine_config() -> GameConfig {
    let letters: Vec<String> = TILE_DISTRIBUTION[1..].iter().map(|(l, _, _)| l.to_string()).collect();
    let scores: Vec<i32> = TILE_DISTRIBUTION[1..].iter().map(|(_, v, _)| *v as i32).collect();
    let counts: Vec<u8> = TILE_DISTRIBUTION[1..].iter().map(|(_, _, c)| *c).collect();
    GameConfig {
        alphabet: Alphabet::new("Hungarian", letters, scores),
        distribution: TileDistribution::new(&counts, TILE_DISTRIBUTION[0].2),
        layout: BoardLayout::standard(),
        rack_size: HAND_SIZE,
        bingo_bonus: BONUS_ALL_TILES,
        min_exchange_bag: 7,
        max_consecutive_zeros: SCORELESS_TURNS_LIMIT,
        ..GameConfig::standard()
    }
}

/// A szó minden olyan zsetonbontása, amelyben nincs „hasított” kétjegyű betű (két szomszédos egybetűs zseton, amely
/// összeolvasva kétjegyű betű lenne: S+Z, C+S, Z+S). Pontosan ezeket a zsetonsorokat fogadja el a játékvezetőnk, ezért a
/// motor a kétjegyű betűk szabályát külön kód nélkül is betartja. A legtöbb szónak egyetlen bontása van; a kevés kétértelmű
/// (pl. EGÉSZSÉG: ...SZ S... és ...S ZS...) mindkét alakban bekerül.
pub fn legal_tilings(word: &str) -> Vec<Vec<Tile>> {
    fn rec(chars: &[char], at: usize, current: &mut Vec<Tile>, out: &mut Vec<Vec<Tile>>) {
        if at == chars.len() {
            out.push(current.clone());
            return;
        }
        for size in 1..=2usize {
            if at + size > chars.len() {
                break;
            }
            let piece: String = chars[at..at + size].iter().collect();
            let Some(tile) = Tile::from_str(&piece) else { continue };
            if tile.is_blank() {
                continue;
            }
            if let Some(previous) = current.last()
                && forms_digraph(previous.as_str(), tile.as_str())
            {
                continue;
            }
            current.push(tile);
            rec(chars, at + size, current, out);
            current.pop();
        }
    }
    let chars: Vec<char> = word.to_uppercase().chars().collect();
    let mut out = Vec::new();
    rec(&chars, 0, &mut Vec::new(), &mut out);
    out
}

/// A motor szótára a robot szókincséből: `(szótár, zsetonsorok száma)`.
pub fn build_lexicon(config: &GameConfig, vocab: &Vocabulary) -> Result<(Lexicon, usize), String> {
    let mut encoded: Vec<Vec<u8>> = Vec::with_capacity(vocab.len() + 1000);
    for word in vocab.iter() {
        for tiling in legal_tilings(word) {
            encoded.push(tiling.iter().map(|t| t.0 - 1).collect());
        }
    }
    let count = encoded.len();
    let lexicon = Lexicon::from_encoded("hu-duel", &config.alphabet, encoded).map_err(|e| format!("a motor szótára nem építhető: {e:?}"))?;
    Ok((lexicon, count))
}
