//! A robot (10. fokozat) és egy független, külső Scrabble motor párharca ugyanazokkal a szabályokkal, ugyanazzal a
//! szószedettel, tükrözött játékpárokban.
//!
//! A külső motor a `third_party/pg-scrabble` (a `scrabble` 0.1.0 crate — pranavgundu, MIT — vendorolt, 64 bites
//! betűmaszkokkal kiszélesített másolata: a magyar ábécé 38 betűjét az eredeti 31-es korlát nem bírta). A játékvezető
//! végig a saját `Game`-ünk: a motor csak lépést javasol, minden lépést a mi szabályaink (szótár, kétjegyű betűk,
//! pontozás, játékvége) hagynak jóvá. Részletek, módszertan és eredmények: `docs/ENGINE_DUEL.md`.
//!
//! | Modul | Feladat |
//! |---|---|
//! | `lexicon` | a motor szabályai (`engine_config`) és szótára: a robot szókincse minden jogos zsetonbontásban |
//! | `bridge` | tábla / kéz / lépés átalakítása a két világ között, a tisztességes „még nem látott” zsetonkészlet |
//! | `sides` | a két oldal: `BotSide` (a robot) és `EngineSide` (a motor: mohó / értékelős / szimulációs / végjáték) |
//! | `referee` | egy játék lejátszása a `Game`-mel: tükrözött zsák, invariánsok, hibakezelés (nincs csendes passz) |
//! | `leaves` | a motor magyar „maradék-értékelése” (lineáris modell) és tanítása önjátékból |
//! | `runner` / `report` / `stats` | párok futtatása szálakon, JSONL napló, párszintű statisztika |
//! | `crosscheck` | a két független lépésgenerátor összevetése véletlen állásokon |

pub mod bridge;
pub mod cpu;
pub mod crosscheck;
pub mod leaves;
pub mod lexicon;
pub mod referee;
pub mod report;
pub mod runner;
pub mod sides;
pub mod spec;
pub mod stats;

/// A kiértékelő játékok magjai ennél kisebbek, a tanítóéi ennél nagyobbak vagy egyenlők (a kettő sosem keveredhet).
pub const EVAL_SEED_LIMIT: u64 = 1_000_000;
pub const TRAIN_SEED_START: u64 = 1_000_000_000;

/// SplitMix64 végső keverés: független magokat származtat egy magból és címkékből.
pub fn mix(mut z: u64) -> u64 {
    z = z.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// Mag származtatása: `seed` és két címke (pl. fél, lépésszám).
pub fn derive(seed: u64, a: u64, b: u64) -> u64 {
    mix(mix(mix(seed) ^ a.wrapping_mul(0xA24B_AED4_963E_E407)) ^ b.wrapping_mul(0x9FB2_1C65_1E98_DF25))
}
