# pg-scrabble: patches against upstream `scrabble` 0.1.0

This directory is a vendored, modified copy of a third-party crate.

| | |
|---|---|
| Upstream package | `scrabble` |
| Upstream version | 0.1.0 (crates.io; upstream VCS commit `a247d31ef957dc11312add8474384ca84c74c9a6`) |
| Upstream repository | <https://github.com/pranavgundu/scrabble> |
| Upstream author | Pranav Gundu <pranav@gundu.me> |
| Upstream licence | MIT (see `LICENSE`, unchanged) |
| This package | `pg-scrabble` 0.1.0, library name `pg_scrabble`, binary `pg-scrabble` |

The MIT licence text in `LICENSE` is kept verbatim; the patches below are
offered under the same terms. This is not an official release of the upstream
project.

## Why

Upstream encodes a letter set as a 32-bit mask (`u32`) and limits an alphabet
to 31 letters (30 once a GADDAG has taken one index for its separator).
Hungarian has 38 letters (seven of them single digraph tiles SZ CS GY LY NY TY
ZS) plus two blanks, so the engine could not be used for it. The patch widens
every letter mask to 64 bits: up to **63 letters** (`MAX_LETTERS`), **62 with a
GADDAG** (`MAX_GADDAG_LETTERS`).

Everything else (algorithms, scoring rules, public API shape, `no_std` core) is
meant to stay as it was. For alphabets that already fitted upstream's limits
the observable behaviour is bit-for-bit identical (see "Verification").

## Capacity arithmetic

* letter index `0..=62` -> bit `l` of a `u64`; bit 63 of a graph header is the
  "accepts" flag.
* GADDAG separator index = `alphabet_len` (<= 62), also a bit in the mask.
* blank tile code `BLANK_CODE = MAX_LETTERS = 63`, `TILE_CODES = 64`
  (`[u8; TILE_CODES]` count arrays in `Rack`, `Bag`, `TileDistribution`, ...).
* `Square` still stores `letter | 0x80` for a blank-as-letter and `0xFF` for
  empty: max letter 62 -> `0xBE`, so no clash (compile-time assertion in
  `tile.rs`).

## Complete list of changes

### Package / naming
* `Cargo.toml`: `name = "pg-scrabble"`, `[lib] name = "pg_scrabble"`,
  `[[bin]] name = "pg-scrabble"`, new description; `documentation` dropped;
  dev-dependency `serde_json` (new serde tests); new `[[test]] wide` and
  `[[example]] stats_wide` entries. `Cargo.toml.orig`, `.cargo-ok`,
  `.cargo_vcs_info.json` removed (registry artefacts).
* `tests/`, `examples/`, `benches/`, `src/bin/`, `README.md`: the path
  `scrabble::` became `pg_scrabble::` (mechanical).

### `src/tile.rs`
* `MAX_LETTERS` 31 -> 63 (so `BLANK_CODE` 31 -> 63, `TILE_CODES` 32 -> 64).
* new `pub(crate) LEGACY_MAX_LETTERS = 31` (see "Compatibility shims").
* compile-time assertions that letters stay below the blank flag / below 64.
* new unit test for the widest letter vs. blank vs. empty.

### `src/lexicon/graph.rs` (the word graph; the heart of the change)
* Node header is now **64 bits stored in two `u32` words**:
  `data[n] | data[n+1] << 32`, bit 63 = accepting, bits 0..=62 = children;
  arcs start at `data[n + 2]`. (Upstream: one header word, bit 31 = accepting,
  arcs from `data[n + 1]`.)
* `children()`, `header()`, `split_header()`, `child_with_mask()`, `ArcIter`
  now use `u64`; `child_for` uses `1u64 << letter`.
* `reordered()`, `validate()` adapted to the 2-word header.
* **Serialised format**: magic `SWG1` -> `SWG2`. `from_bytes` still accepts
  `SWG1` (files written by upstream) and widens them on load (`widen_v1`,
  breadth-first, producing exactly the layout the builder produces). `SWG2`
  is never produced by upstream, and upstream cannot read it (clean
  `BadMagic`). The `SLEX` lexicon wrapper is unchanged (it stores
  `alphabet_len`/`separator` as bytes and embeds two graphs, each carrying its
  own magic), so old `.slex` files load and new ones with alphabet <= 30 differ
  from old ones only inside the graphs.
* tests: arcs above letter 31 in every accessor, accept bit vs highest letter,
  `SWG1` upstream-format loading (a test-local writer of the old layout),
  corruption handling. One upstream test poked `data[root + 1]` as "the first
  arc"; it now pokes `data[root + 2]`.

### `src/lexicon/builder.rs`
* mask/header built as `u64`; two words pushed per node; structural hash and
  `matches()` compare both words; `with_capacity` reserves `words * 5`
  (was `* 4`). Node de-duplication is unchanged, so graphs are structurally
  identical (same node/arc counts) for old alphabets.
* test: a node with arcs on both sides of the old 32-bit boundary; the first
  index past the limit is rejected.

### `src/lexicon/mod.rs`
* `MAX_GADDAG_LETTERS` follows `MAX_LETTERS` (30 -> 62).
* `Lexicon::letter_mask()` and `Lexicon::cross_set()` return `u64`.
* `anagram_walk` uses the `u64` child mask (via `children()`).
* the `SLEX` header is unchanged (see above).

### `src/rack.rs`
* `Rack.mask: u32 -> u64`; `mask()`, `playable_mask()` use `u64`;
  `1u64 << letter` everywhere.
* `Default` is now implemented by hand: `[u8; N]: Default` only exists up to
  N = 32.
* `Rack::key()` (used by `LeaveTable`) keeps the old numbering: the blank
  hashes as code 31 and letters >= 31 (which did not exist) as code + 1, so
  keys of racks over old alphabets are unchanged and leave tables trained by
  upstream (`SLV1` files) still match. Tests pin concrete upstream values.

### `src/movegen.rs`
* `Analysis::cross` is `Vec<u64>`, `Analysis::cross_set()` returns `u64`;
  `Gen::sep_bit`, `cross`, `for_each_placement(allowed)`, `extend_right`,
  `walk_left` use `u64`. The lane-offset masks `placed` / `from_rack` stay
  `u32` on purpose (they index board columns, `MAX_WORD = 32`, not letters).
  `internal::bits(u32)` is therefore unchanged.

### `src/rules.rs`
* `Alphabet::full_mask()` returns `u64`.
* `TileDistribution` derive(serde) cannot handle `[u8; 64]` -> `#[serde(with =
  "counts_serde")]`. Wire format: the old 32 entries (letters 0..=30, then the
  blank), plus 32 more entries (letters 31..=62) only when one is non-zero.
  So a distribution of an old-sized alphabet serialises byte-for-byte as in
  upstream, and old JSON still deserialises (test pins the English JSON).

### `src/endgame.rs`
* The Zobrist table is sized for `MAX_LETTERS`. To keep every key (hence every
  transposition-table hit/collision and node count) identical to upstream for
  alphabets <= 31 letters, the original random stream is drawn first in the
  original order/shape, the extra keys for letters 31..62 afterwards, and the
  values are flattened into tables indexed by the new numbering (no branch in
  the hot lookup). `key()` now walks the rack's presence mask instead of all
  tile codes. Tests include a verbatim copy of the old key function as oracle.

### Untouched but audited (nothing to change)
`bag.rs`, `board.rs`, `game.rs`, `eval.rs`, `infer.rs`, `sim.rs`, `train.rs`,
`rng.rs`, `formats/*`, `src/bin/scrabble.rs`: only use `TILE_CODES` /
`BLANK_CODE` symbolically, or fixed 26-letter English helpers. Tile order in
the bag is by code, letters before the blank in both numberings, so bag
shuffles are identical.

### Documentation / examples / tests added
* `README.md`: capacity statements and graph layout updated.
* `examples/stats_wide.rs` (38-letter Hungarian version of `stats`).
* `tests/wide.rs` (18 tests; see below).

## Public API differences (a user of the old crate must adapt)
| item | upstream | patched |
|---|---|---|
| `tile::MAX_LETTERS`, `BLANK_CODE`, `TILE_CODES` | 31, 31, 32 | 63, 63, 64 |
| `lexicon::MAX_GADDAG_LETTERS` | 30 | 62 |
| `WordGraph::{children, header, child_with_mask, split_header}`, `ArcIter` | `u32` | `u64` |
| `WordGraph::raw()` layout; `WordGraph::MAGIC` | 1 header word; `SWG1` | 2 header words; `SWG2` (`MAGIC_V1` reads old) |
| `Lexicon::{letter_mask, cross_set}`, `Analysis::cross_set` | `u32` | `u64` |
| `Rack::{mask, playable_mask}`, `Alphabet::full_mask` | `u32` | `u64` |
| `Rack: Default` | derived | manual (same value) |
| `TileDistribution` serde | `[u8;32]` | compatible, see above |

## Verification

All commands use `CARGO_TARGET_DIR=<shared target>`.

1. **Upstream suite, before** (`cargo test --all-features` in the pristine
   copy): 142 unit + 7 `game` + 16 `movegen` + 2 doc = **167 passed, 0 failed**.
2. **Upstream suite, after** (patched crate, same command): all 167 names pass
   (the only rename is the README doctest's line number, 293 -> 295), plus
   **29 new tests** -> **196 passed, 0 failed**:
   153 unit (142 + 11 new), 7 `game`, 16 `movegen`, 18 `wide`, 2 doc.
   Also green: `cargo test` (default features), `cargo test
   --no-default-features --lib` (106 tests; 97 upstream), `cargo build
   --no-default-features --lib` (the `no_std` core), `cargo clippy
   --all-features --all-targets` (no warnings, same as upstream),
   `cargo fmt --check`.
3. **Naive-vs-fast move generator equivalence on wide alphabets**
   (`tests/wide.rs`, using `tests/common/reference.rs`, the naive generator
   that enumerates the whole word list):
   * 38-letter Hungarian tile set (letters/points/counts copied from
     `src/engine/tiles.rs` `TILE_DISTRIBUTION`, labels SZ CS GY LY NY TY ZS),
     small made-up Hungarian-looking lexicon (~500 words, all 38 letters
     occur), 9x9 board, rack 5, 14 random games x 10 turns = 140 positions with
     racks built to contain wide letters and blanks. Measured coverage in the
     run: 3,725 plays, 78 positions with a blank in the rack, 3,272 plays that
     use a blank, 1,675 plays that place a letter >= 31, 989 plays where a blank
     stands for a letter >= 31, 540 plays through a wide tile already on the
     board. Fast generator and naive generator agree on every position
     (same set of plays with identical placements and scores, no duplicates).
   * explicit blank positions (one/two blanks, with CS Ő Ú Ű LY ZS TY, opening
     and non-opening boards), blank-vs-plain scoring, digraph tile = one square.
   * a synthetic **62-letter** alphabet (the maximum; separator = 62) with
     900+ words over the whole range, blanks included: 8 games x 8 turns.
   * parallel (rayon) generation equals sequential on Hungarian.
   * 63 letters is refused with `AlphabetTooLarge { len: 63, max: 62 }`.
   * Mutation check: truncating the child mask or the cross set to 32 bits
     makes 10 resp. 3 of these tests fail.
4. **Lexicon (de)serialisation over 38 letters**
   (`a_38_letter_lexicon_survives_serialisation`): `to_bytes` -> `from_bytes`
   gives equal DAWG and GADDAG, `validate()` passes, identical move generation,
   truncation rejected; plus graph-level tests for `SWG2` and for loading
   upstream's `SWG1`.
5. **Bit-for-bit identity for <= 30 letters** (`../diffcheck`, a crate that links
   both upstream and the patch and runs one workload through each): English
   word list of 287,589 words; compares lexicon structure (node/arc counts,
   hash of every DAWG and GADDAG word), cross sets, anagrams, `Rack::key`,
   bag draw order, **every play of every position in emission order** (30
   greedy self-play games with `StaticEvaluator`, ~5.1 million plays hashed),
   final scores, 12 endgame solves (spread, **node counts**, exactness, best
   play), 5 simulator runs (play, mean, static equity to the last bit), and
   that an upstream-written `.slex` loads in the patched crate and equals the
   freshly built lexicon. Result: `IDENTICAL: all 54 output lines ... match
   byte for byte` (3 runs; outputs in `../out/`).

## Cost of the widening (English 287,589-word list, release build)

| | upstream | patched | change |
|---|---|---|---|
| DAWG | 100,664 nodes, 281,329 arcs, 1,527,972 B | same nodes/arcs, 1,930,628 B | +26.4 % |
| GADDAG | 582,057 nodes, 1,355,868 arcs, 7,751,700 B | same, 10,079,928 B | +30.0 % |
| resident lexicon (DAWG + GADDAG) | 9.28 MB | 12.01 MB | +2.73 MB |
| serialised `.slex` | 9,279,759 B | 12,010,643 B | +29.4 % |
| `size_of::<Rack>()` | 40 B | 80 B | x2 |
| peak RSS, build + generate (`examples/stats`) | 247.8 MiB | 255.4 MiB | +3 % |
| lexicon build time | 2.87 s | 2.97 s | within noise |
| move generation (`examples/stats`, sum of per-row minima over 12 rows, 8 runs) | 66.4 ms | 69.3 ms | +4.5 % |
| whole diffcheck workload (30 games + 12 endgames + 5 sims), 3 runs | 11.29 / 11.23 / 11.60 s | 11.56 / 11.89 / 11.85 s | +2 .. +6 % |

The memory cost is exactly one extra `u32` word per graph node (582,057 +
100,664 nodes x 4 B = 2.73 MB). A variable-width header (one word when no arc
index exceeds 30) would remove it for narrow alphabets at the price of a branch
in the hot loop; not done, because it adds complexity and the 38-letter case
needs the wide header anyway.

Hungarian proxy lexicon (`examples/stats_wide`, 260,989 words: wordfreq-hu union
the repo's `hu_HU.dic` stems plus synthetic suffixed forms; upstream cannot build
it): DAWG 100,742 nodes / 290,223 arcs / 1.9 MiB, GADDAG 651,819 nodes /
1,449,525 arcs / 10.5 MiB; compile 2.2 s.

## Known limitations / things to know when driving the engine with Hungarian

* `Alphabet::parse_char`, `Rack::parse`, `Board::parse`, `Lexicon::from_words`
  are **single-character** parsers: they would turn the text "SZ" into S + Z.
  For multi-character labels build words with your own tokeniser and use
  `Lexicon::from_encoded(name, &alphabet, Vec<Vec<u8>>)` and index-based
  `Tile::letter(i)` / `Square::letter(i)` / `Board::set`. (`tests/wide.rs`
  does exactly this.) `parse_char` is also ASCII-case based, so lowercase
  accented letters are not recognised as blanks.
* The engine has **no digraph rule**: S + Z on adjacent squares is a legal
  play here if the word is in the lexicon (the Hungarian referee forbids it).
* `eval::heuristic_leave` only knows English letter values; for other alphabets
  it applies the blank bonus (+24) and a duplicate penalty, no per-letter or
  vowel/consonant terms. For a fair Hungarian duel use a `LeaveTable` trained
  with the crate's `train` feature (`train_leaves`) or `StaticEvaluator::greedy()`.
* `Display for Rack` and `Debug for Play` are English (A-Z) cosmetics; use
  `Rack::to_text(&alphabet)` / `Play::word_text(&alphabet)`.
* The patched crate reads `SWG1` graphs / `.slex` files written by upstream but
  only writes `SWG2`, which upstream cannot read.

## Reproduce

```sh
export CARGO_TARGET_DIR=<shared target>
cd upstream     && cargo test --all-features      # baseline (pristine copy)
cd pg-scrabble  && cargo test --all-features      # patched
cd diffcheck    && cargo build --release && \
  $CARGO_TARGET_DIR/release/diffcheck ../data/english.txt 30 ../out
cd pg-scrabble  && cargo build --release --example stats_wide && \
  SCRABBLE_LEXICON=../data/hungarian.txt $CARGO_TARGET_DIR/release/examples/stats_wide
diff -ruN upstream pg-scrabble > pg-scrabble.diff
```
