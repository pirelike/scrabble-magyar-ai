# pg-scrabble (patched scrabble 0.1.0 — see PATCHES.md)

A fast, complete crossword-game engine in Rust: move generation, scoring, the
full rule set, simulation, and exact endgame search.

```rust
use pg_scrabble::prelude::*;

let config = GameConfig::standard();
let lexicon = Lexicon::from_words(&config.alphabet, ["CAT", "CATS", "AT", "SAT"])?;

let board = Board::new(&config.layout);
let rack = Rack::parse(&config.alphabet, "CATS").unwrap();

let mut generator = MoveGenerator::new(&config);
for play in generator.generate_sorted(&board, &rack, &lexicon).iter().take(3) {
    println!("{}", play.to_text(&config.alphabet));
}
# Ok::<(), pg_scrabble::lexicon::LexiconError>(())
```

## Install

```toml
[dependencies]
pg-scrabble = { path = "../pg-scrabble" }
```

Requires Rust 1.85. The `no_std` core compiles on 1.75; the higher floor comes
from optional dependencies (rayon needs 1.80, clap needs 1.85), so
`--no-default-features` reaches further back.

## What's included

* Board, rack, bag and rules. Any board size, any alphabet, any tile
  distribution. Variants are configuration, not code.
* Lexicon compiler. Turns any word list into a minimised DAWG and GADDAG.
* Move generation. Exhaustive, and verified against two independent
  implementations.
* Game state. Turn order, exchanges, passes, six-zero termination,
  end-of-game adjustments, full undo.
* Evaluation. Score plus leave value, with a built-in heuristic and a
  self-play trainer.
* Simulation. Monte-Carlo rollouts with common random numbers and sequential
  halving.
* Endgame. Exact alpha-beta with a Zobrist transposition table.
* GCG transcripts, read and written.
* A CLI: `build`, `analyze`, `endgame`, `play`, `anagram`, `train-leaves`,
  `compare`, `bench`.

No `unsafe`. The core is `no_std` plus `alloc`, and CI builds it for
`thumbv7em-none-eabihf` to prove it.

## Word lists are not included

Tournament lexicons such as TWL and Collins are copyrighted, so this crate
ships the compiler rather than the data.

```sh
scrabble build my-word-list.txt -o my-lexicon.slex
scrabble analyze -l my-lexicon.slex -r AEINRST
```

Any newline-separated list works. Most Unix systems have
`/usr/share/dict/words` if you need one to try.

## Speed

Measured on an Apple M-series laptop against `/usr/share/dict/words`, which is
227,624 words: larger than TWL and comparable to Collins.

```text
compile          1.2 s
dawg             116,132 nodes    1.5 MiB
gaddag           803,946 nodes    9.3 MiB
```

Against [Quackle](https://github.com/quackle/quackle), on the same word list
and the same board positions, timing generate + score + sort on both sides:

| Position | Rack | Plays | Quackle | scrabble | Speedup |
|---|---|---:|---:|---:|---:|
| early | AEINRST | 1,797 | 1231 us | 636 us | 1.94x |
| early | AEIOUUV | 273 | 139 us | 46 us | 3.03x |
| early | QJXZKWV | 17 | 8.0 us | 4.1 us | 1.95x |
| early | AEINRS? | 13,410 | 9218 us | 5676 us | 1.62x |
| midgame | AEINRST | 931 | 568 us | 220 us | 2.58x |
| midgame | AEIOUUV | 183 | 80 us | 26 us | 3.13x |
| midgame | QJXZKWV | 6 | 5.0 us | 2.7 us | 1.85x |
| midgame | AEINRS? | 6,906 | 4357 us | 2014 us | 2.16x |
| total | | | 15.6 ms | 8.6 ms | 1.81x |

So 1.6x to 3.1x single-threaded. Memory is the wider margin: Quackle's GADDAG
over the same words is 32 MB against 9.3 MiB here, 3.4x smaller.

The Plays column is a single number because both engines agree on it exactly
in all eight positions. That matters more than the timings. Quackle is a mature
engine with an independent implementation of the same rules, so identical
enumeration is strong evidence the generator here is correct.

Opening positions are left out of the table. There scrabble returns exactly
twice as many plays, because on an empty board it counts a horizontal placement
and its vertical mirror as two plays while Quackle collapses the pair. Neither
is wrong, but they are not comparable.

`bench/quackle/run.sh` reproduces all of this. It clones Quackle, compiles a
Quackle lexicon from the same word list, runs both engines over the same
boards, and checks that the boards really are identical before reporting
anything.

scrabble has not been benchmarked against
[wolges](https://github.com/andy-k/wolges), so no comparison is claimed there.

## Using more cores

The 30 lane scans in one generation are independent, so
`Prepared::generate_parallel` runs them across a rayon pool. Output is
byte-identical to the sequential path, which the test suite asserts over
randomised games.

On 8 threads:

| Position | Rack | Plays | Serial | Parallel | Speedup |
|---|---|---:|---:|---:|---:|
| early | AEINRS? | 13,410 | 4334 us | 1341 us | 3.23x |
| midgame | AEINRS? | 6,906 | 1391 us | 538 us | 2.59x |
| early | AEINRST | 1,797 | 390 us | 223 us | 1.75x |
| midgame | AEINRST | 931 | 123 us | 98 us | 1.25x |
| midgame | AEIOUUV | 183 | 14.5 us | 30.6 us | 0.48x |
| midgame | QJXZKWV | 6 | 2.6 us | 20.8 us | 0.13x |

Note the last two rows. Handing work to the pool costs about 20 us, so below
roughly a thousand plays parallelism is a loss. Which case you are in cannot be
predicted cheaply, since `QJXZKWV` and `AEINRST` look identical to any static
estimate and differ by 50x in cost. So this is opt-in rather than automatic.
Use it for analysing a heavy position, not inside a simulation, which is
already parallel one level up.

## How move generation works

Gordon's GADDAG algorithm, with one change that carries most of the speed.

A GADDAG stores, for every way of splitting a word, the first part reversed
followed by a separator and then the rest. That lets a generator start at an
anchor square in the middle of a word, read leftwards to the word's start, turn
around, and read rightwards to its end.

The usual packing stores each arc as one machine word and finds the arc for a
letter by scanning the node's run of arcs. That scan is the hottest loop in the
engine. Here each node instead carries a bitmask of which letters it has
(pg-scrabble: 64-bit, upstream: `u32`) arcs for, followed immediately by its targets in letter order:

```text
data[n], data[n+1]          64-bit header: bit 63 = accepting, bits 0..62 = child letters
data[n+2 .. n+2+k]          k = popcount(children) targets, ascending by letter
(upstream 0.1.0, one header word: data[n] bit 31 = accepting, bits 0..30 = child
letters, targets from data[n+1]; see PATCHES.md)
```

Finding the arc for letter `L` becomes one mask, one popcount and one load:

```rust,ignore
data[n + 2 + (children & ((1u64 << L) - 1)).count_ones()]   // upstream: data[n + 1 + ...]
```

No scan and no branch. Better still, the caller can intersect three bitmasks
before touching memory:

```rust,ignore
graph.children(node) & analysis.cross_set(square) & rack.mask()
```

so the generator only descends into letters that exist in the lexicon, form a
legal perpendicular word, and are backed by a tile in hand. Nothing is tried
and rejected.

## Correctness

Move generation is the part that must not be wrong, and "it looks right" is not
evidence. The test suite contains a second, deliberately naive generator that
loops over every word in the lexicon and tries it at every position, checking
the rules one at a time. It shares no logic with the real one.

`tests/movegen.rs` runs both on hand-built positions and on positions reached
by playing out randomised games, and asserts the two produce exactly the same
plays with exactly the same scores. `tests/game.rs` plays full games and checks
that tiles are conserved, that undo is a true inverse, and that no points are
invented. Quackle agreeing on play counts is a third, independent check.

## Playing strength

The built-in leave heuristic is worth about twenty points a game over pure
greed, measured over mirrored game pairs:

```text
$ scrabble compare -l words.slex --games 120 --vs-greedy
  A 71 - 49 B, 0 tied
  win rate    59.2%
  mean spread +21.6 per game
```

Games are played in mirrored pairs, the same bag twice with each side moving
first, so the deal cancels arithmetically rather than statistically. Two
identical evaluators score a dead-even zero, which is the harness's own
self-check.

Strong engines use superleaves: leave values fitted from millions of self-play
games. The well-known tables carry licences this crate cannot adopt, so there
is a trainer instead:

```sh
scrabble train-leaves -l my-lexicon.slex -o leaves.slv --games 20000
scrabble compare -l my-lexicon.slex --leaves leaves.slv --games 500
```

Fit your own, measure it against the heuristic, and keep it if it wins.

## Endgames are solved, not guessed

Once the bag is empty both racks are known and the rest of the game is finite
and perfect-information, so `EndgameSolver` searches it exactly.

```text
$ scrabble endgame -l words.slex -b position.txt -r SE -o QZ
best:   9H ES
spread: +50
solved exactly in 0.00s over 50 positions
```

Note the factor of two. Playing out adds the opponent's rack to your score and
subtracts it from theirs, so being stuck with `QZ` is a forty-point swing, not
twenty. That is why a ten-point play that goes out beats a thirty-point play
that does not, and getting it wrong is the most common way an endgame evaluator
goes astray.

## Feature flags

Default: `std`, `ai`, `formats`, `rayon`.

| Flag | What it adds |
|---|---|
| `std` | Turning it off gives a `no_std` + `alloc` core: board, rack, lexicon compilation and queries, move generation. |
| `ai` | Evaluation, simulation, endgame search, inference. |
| `formats` | GCG. |
| `train` | The leave trainer and the evaluator match harness. |
| `rayon` | `Prepared::generate_parallel`, and simulation across candidates. |
| `serde` | Derives on the state types. |
| `cli` | The `scrabble` binary. |

WASM and Python bindings are not here yet. The core is `no_std` with no C
dependencies, so both would be straightforward wrapper crates.

## Variants

Board layouts are strings, so a new variant needs no code:

```rust
use pg_scrabble::prelude::*;

let layout = BoardLayout::parse("my board", "\
T..d...T
.D.....D
..d...d.
d..D.D..
...DD...
..d...d.
.D.....D
T..d...T")?;

let config = GameConfig { layout, rack_size: 6, ..GameConfig::standard() };
# Ok::<(), pg_scrabble::rules::LayoutError>(())
```

`.` is a normal square, `d`/`t`/`q` are letter premiums, `D`/`T`/`Q` are word
premiums. Alphabets and tile distributions are data too, up to 63 letters (pg-scrabble; upstream: 31), or
62 with a GADDAG, which reserves one index for its separator.

## Benchmarks

```sh
cargo run --release --example stats          # lexicon size and throughput
cargo run --release --example par_bench      # serial vs parallel
bench/quackle/run.sh                         # the Quackle comparison
cargo bench                                  # criterion
```

## Licence

MIT. See [LICENSE](LICENSE). No bundled word list, so nothing here carries
anyone else's terms.
