#![cfg_attr(not(feature = "std"), no_std)]
#![warn(clippy::all)]
#![forbid(unsafe_code)]

extern crate alloc;

#[cfg(feature = "std")]
extern crate std;

pub mod bag;
pub mod board;
pub mod lexicon;
pub mod movegen;
pub mod rack;
pub mod rng;
pub mod rules;
pub mod tile;

pub mod game;

#[cfg(feature = "ai")]
pub mod endgame;
#[cfg(feature = "ai")]
pub mod eval;
#[cfg(feature = "ai")]
pub mod infer;
#[cfg(feature = "ai")]
pub mod sim;

#[cfg(feature = "formats")]
pub mod formats;

#[cfg(feature = "train")]
pub mod train;

pub(crate) mod internal;

#[cfg(doctest)]
#[doc = include_str!("../README.md")]
pub struct ReadmeExamples;

#[cfg(test)]
#[allow(dead_code)]
pub(crate) fn lexicon_test_words() -> alloc::vec::Vec<&'static str> {
    "AT ATE EAT EATS TEA TEAS SEA SEAT SEATS SET SETA CAT CATS CAST CASE ACE ACES
     SAT SATE TA TAE TAS AS ES AE EA ST STAT STATE ESTATE CASTE CATES TACE TACES
     ACT ACTS CAST CASTS SCAT SCATS TACT EAST EASTS ETA ETAS TEAT TEATS SEAT
     STONE TONES NOTES ONSET SETON STENO TONE NOTE ONE ONES NOSE NOSES SON SONS
     RETAIN RETINA RATINE AEON AEONS AIT AITS ANTE ANTES NEAT NEATS TAN TANS
     AN NA NAE ANE ANES EN ENS NE OS SO OSE OSES TO OT NOT TON NOTS TONS
     AB BA BAT BATS TAB TABS BEAT BEATS ABET ABETS BASE BASES BAST BASTE
     QI ZA XI AX AXE AXES JO JOE JOES OX BOX BOXES"
        .split_whitespace()
        .collect()
}

pub mod prelude {
    pub use crate::bag::Bag;
    pub use crate::board::{Board, Coord, Direction};
    pub use crate::game::{Game, GameEnd, Player, Turn, TurnError};
    pub use crate::lexicon::{GraphBuilder, Lexicon};
    pub use crate::movegen::{Analysis, MoveGenerator, Play, PlayError, Prepared};
    pub use crate::rack::Rack;
    pub use crate::rules::{
        Alphabet, BoardLayout, ChallengeRule, GameConfig, Premium, TileDistribution,
    };
    pub use crate::tile::{Square, Tile};

    #[cfg(feature = "ai")]
    pub use crate::endgame::{EndgameResult, EndgameSolver};
    #[cfg(feature = "ai")]
    pub use crate::eval::{EvalContext, Evaluator, StaticEvaluator, Unseen};
    #[cfg(feature = "ai")]
    pub use crate::infer::Inference;
    #[cfg(feature = "ai")]
    pub use crate::sim::{SimOptions, SimResult, Simulator};

    #[cfg(all(feature = "ai", feature = "std"))]
    pub use crate::eval::LeaveTable;
}
