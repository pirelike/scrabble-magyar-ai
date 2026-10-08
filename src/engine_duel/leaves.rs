//! A motor magyar „maradék-értékelése”: egy lineáris modell, amelyet a motor önjátékából tanítunk.
//!
//! A kézben maradó zsetonok (a „maradék”) értéke = Σ zsetononkénti érték + Σ párok szinergiája (azonos betűk
//! ismétlődése is). A tanítóadat: a motor önjátékában minden középjátékbeli lerakás maradéka és az ebből a
//! pillanatból a játék végéig elért pontkülönbség (a lerakás utáni állástól számolva). A közös tagot (átlagos
//! folytatás) a modell tengelymetszete viszi el, ezért az üres maradék értéke 0.
//!
//! Ez **független** a robot kézzel írt `leave_value`-jától: a tanítás a motor saját játékából jön, a robot
//! sosem szerepel benne, a magok pedig a kiértékelő párharcok magjaitól elkülönülnek (`TRAIN_SEED_START`).

use super::{TRAIN_SEED_START, derive, mix};
use pg_scrabble::eval::{EvalContext, Evaluator};
use pg_scrabble::game::{Game, Turn};
use pg_scrabble::movegen::{MoveGenerator, Play};
use pg_scrabble::prelude::*;
use pg_scrabble::rng::Rng;
use pg_scrabble::tile::BLANK_CODE;
use serde_json::{Value, json};
use std::sync::atomic::{AtomicU64, Ordering};

/// A zsetontípusok helye: 0..=37 a betűk, 38 a joker.
pub const TILE_SLOTS: usize = 39;
const PAIRS: usize = TILE_SLOTS * (TILE_SLOTS + 1) / 2;
pub const N_FEATURES: usize = TILE_SLOTS + PAIRS + 1;
const BIAS: usize = N_FEATURES - 1;
const BLANK_SLOT: usize = TILE_SLOTS - 1;

fn pair_index(a: usize, b: usize) -> usize {
    debug_assert!(a <= b);
    TILE_SLOTS + b * (b + 1) / 2 + a
}

/// A maradék jellemzői `(index, érték)` párokként, növekvő index szerint (a tengelymetszet nincs benne).
pub fn features(leave: &Rack, out: &mut Vec<(usize, f64)>) {
    out.clear();
    let mut present: [(usize, f64); TILE_SLOTS] = [(0, 0.0); TILE_SLOTS];
    let mut n = 0;
    let counts = leave.counts();
    for slot in 0..TILE_SLOTS {
        let code = if slot == BLANK_SLOT { BLANK_CODE as usize } else { slot };
        let c = counts[code];
        if c > 0 {
            present[n] = (slot, c as f64);
            n += 1;
        }
    }
    for &(slot, count) in &present[..n] {
        out.push((slot, count));
    }
    for (i, &(a, na)) in present[..n].iter().enumerate() {
        for &(b, nb) in &present[i..n] {
            let value = if a == b { na * (na - 1.0) / 2.0 } else { na * nb };
            if value > 0.0 {
                out.push((pair_index(a, b), value));
            }
        }
    }
    out.sort_by_key(|f| f.0);
}

#[derive(Clone, Debug)]
pub struct LinearLeaves {
    pub weights: Vec<f64>,
}

impl LinearLeaves {
    pub fn zero() -> LinearLeaves {
        LinearLeaves { weights: vec![0.0; N_FEATURES] }
    }

    pub fn value(&self, leave: &Rack) -> f64 {
        let mut feats = Vec::with_capacity(32);
        features(leave, &mut feats);
        feats.iter().map(|(i, v)| self.weights[*i] * v).sum()
    }

    pub fn to_json(&self, meta: Value) -> Value {
        json!({"version": 1, "features": N_FEATURES, "meta": meta, "weights": self.weights})
    }

    pub fn from_json(value: &Value) -> Result<LinearLeaves, String> {
        let weights: Vec<f64> = value["weights"].as_array().ok_or("hiányzó weights")?.iter().map(|v| v.as_f64().unwrap_or(f64::NAN)).collect();
        if weights.len() != N_FEATURES || weights.iter().any(|w| !w.is_finite()) {
            return Err(format!("a leave-fájl {} súlyt tartalmaz, {} kellene (vagy nem véges szám)", weights.len(), N_FEATURES));
        }
        Ok(LinearLeaves { weights })
    }

    pub fn load(path: &std::path::Path) -> Result<LinearLeaves, String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let value: Value = serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
        LinearLeaves::from_json(&value)
    }
}

impl Evaluator for LinearLeaves {
    fn equity(&self, ctx: &EvalContext<'_>, play: &Play) -> f64 {
        let Some(leave) = play.leave(ctx.rack) else { return f64::NEG_INFINITY };
        let score = play.score() as f64;
        if ctx.bag_remaining == 0 {
            // ugyanaz a végjáték-alak, mint a motor gyári értékelésénél: a kézben maradt zsetonok pontja levonódik
            let stuck = leave.value(&ctx.config.alphabet) as f64;
            return score - stuck + if leave.is_empty() { 10.0 } else { 0.0 };
        }
        score + self.value(&leave)
    }

    fn leave_value(&self, _alphabet: &Alphabet, leave: &Rack) -> f64 {
        self.value(leave)
    }
}

// ===================================================================================================
// Legkisebb négyzetek (gerinc-regresszió)
// ===================================================================================================

/// A normálegyenlet összegzői: Σ x xᵀ (a felső háromszög), Σ x y.
pub struct Normal {
    xtx: Vec<f64>,
    xty: Vec<f64>,
    pub samples: u64,
    sum_y: f64,
    sum_yy: f64,
}

impl Normal {
    pub fn new() -> Normal {
        Normal { xtx: vec![0.0; N_FEATURES * N_FEATURES], xty: vec![0.0; N_FEATURES], samples: 0, sum_y: 0.0, sum_yy: 0.0 }
    }

    /// Egy megfigyelés: a (növekvő indexű) jellemzők és a kimenet; a tengelymetszet jellemzőjét maga adja hozzá.
    pub fn add(&mut self, feats: &[(usize, f64)], y: f64) {
        let n = feats.len();
        for a in 0..n {
            let (i, vi) = feats[a];
            self.xty[i] += vi * y;
            let row = i * N_FEATURES;
            for &(j, vj) in &feats[a..n] {
                self.xtx[row + j] += vi * vj;
            }
            self.xtx[row + BIAS] += vi;
        }
        self.xtx[BIAS * N_FEATURES + BIAS] += 1.0;
        self.xty[BIAS] += y;
        self.samples += 1;
        self.sum_y += y;
        self.sum_yy += y * y;
    }

    pub fn merge(&mut self, other: &Normal) {
        for (a, b) in self.xtx.iter_mut().zip(&other.xtx) {
            *a += b;
        }
        for (a, b) in self.xty.iter_mut().zip(&other.xty) {
            *a += b;
        }
        self.samples += other.samples;
        self.sum_y += other.sum_y;
        self.sum_yy += other.sum_yy;
    }

    pub fn mean_y(&self) -> f64 {
        if self.samples == 0 { 0.0 } else { self.sum_y / self.samples as f64 }
    }

    pub fn sd_y(&self) -> f64 {
        if self.samples < 2 {
            return 0.0;
        }
        let n = self.samples as f64;
        ((self.sum_yy - self.sum_y * self.sum_y / n) / (n - 1.0)).max(0.0).sqrt()
    }

    /// A megoldás gerinc-büntetéssel (`lambda`, a tengelymetszetet nem büntetjük). A tengelymetszet nem része a
    /// visszaadott modellnek.
    pub fn solve(&self, lambda: f64) -> Result<LinearLeaves, String> {
        let mut leaves = self.solve_full(lambda)?;
        leaves.weights[BIAS] = 0.0;
        Ok(leaves)
    }

    /// Mint a `solve`, de a tengelymetszet is a súlyok között marad (az utolsó elem): a hibamérésekhez kell.
    pub fn solve_full(&self, lambda: f64) -> Result<LinearLeaves, String> {
        let n = N_FEATURES;
        let mut a = vec![0.0; n * n];
        for i in 0..n {
            for j in i..n {
                let v = self.xtx[i * n + j];
                a[i * n + j] = v;
                a[j * n + i] = v;
            }
            if i != BIAS {
                a[i * n + i] += lambda;
            }
        }
        let mut b = self.xty.clone();
        cholesky_solve(&mut a, &mut b, n)?;
        Ok(LinearLeaves { weights: b })
    }

    /// Négyzetes átlaghiba a megadott (tengelymetszettel együtt vett) súlyokra ezen az adathalmazon.
    pub fn mse(&self, full: &LinearLeaves) -> f64 {
        if self.samples == 0 {
            return f64::NAN;
        }
        let n = N_FEATURES;
        let w = &full.weights;
        let mut quad = 0.0;
        for i in 0..n {
            quad += w[i] * w[i] * self.xtx[i * n + i];
            for j in i + 1..n {
                quad += 2.0 * w[i] * w[j] * self.xtx[i * n + j];
            }
        }
        let cross: f64 = (0..n).map(|i| w[i] * self.xty[i]).sum();
        (self.sum_yy - 2.0 * cross + quad) / self.samples as f64
    }

    /// Mentés bináris fájlba (újraoldáshoz és körök egyesítéséhez).
    pub fn save(&self, path: &std::path::Path) -> Result<(), String> {
        let mut bytes: Vec<u8> = Vec::with_capacity(8 * (self.xtx.len() + self.xty.len() + 8));
        bytes.extend_from_slice(b"NRM1");
        bytes.extend_from_slice(&(N_FEATURES as u64).to_le_bytes());
        bytes.extend_from_slice(&self.samples.to_le_bytes());
        bytes.extend_from_slice(&self.sum_y.to_le_bytes());
        bytes.extend_from_slice(&self.sum_yy.to_le_bytes());
        for v in self.xtx.iter().chain(self.xty.iter()) {
            bytes.extend_from_slice(&v.to_le_bytes());
        }
        std::fs::write(path, bytes).map_err(|e| format!("{}: {e}", path.display()))
    }

    pub fn load(path: &std::path::Path) -> Result<Normal, String> {
        let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let header = 4 + 8 * 4;
        let expected = header + 8 * (N_FEATURES * N_FEATURES + N_FEATURES);
        if bytes.len() != expected || &bytes[..4] != b"NRM1" {
            return Err(format!("{}: nem normálegyenlet-fájl (vagy más jellemzőkészlethez készült)", path.display()));
        }
        let u64_at = |at: usize| u64::from_le_bytes(bytes[at..at + 8].try_into().unwrap());
        let f64_at = |at: usize| f64::from_le_bytes(bytes[at..at + 8].try_into().unwrap());
        if u64_at(4) != N_FEATURES as u64 {
            return Err(format!("{}: {} jellemzős fájl, {} kellene", path.display(), u64_at(4), N_FEATURES));
        }
        let mut normal = Normal::new();
        normal.samples = u64_at(12);
        normal.sum_y = f64_at(20);
        normal.sum_yy = f64_at(28);
        let mut at = header;
        for slot in normal.xtx.iter_mut().chain(normal.xty.iter_mut()) {
            *slot = f64_at(at);
            at += 8;
        }
        Ok(normal)
    }
}

impl Default for Normal {
    fn default() -> Self {
        Normal::new()
    }
}

/// Szimmetrikus, pozitív definit rendszer megoldása Cholesky-felbontással (helyben; az eredmény `b`-ben).
fn cholesky_solve(a: &mut [f64], b: &mut [f64], n: usize) -> Result<(), String> {
    for j in 0..n {
        let mut d = a[j * n + j];
        for k in 0..j {
            d -= a[j * n + k] * a[j * n + k];
        }
        if d <= 1e-12 {
            return Err(format!("a normálegyenlet nem pozitív definit a(z) {j}. jellemzőnél (túl kevés adat?)"));
        }
        let d = d.sqrt();
        a[j * n + j] = d;
        for i in j + 1..n {
            let mut s = a[i * n + j];
            for k in 0..j {
                s -= a[i * n + k] * a[j * n + k];
            }
            a[i * n + j] = s / d;
        }
    }
    for i in 0..n {
        let mut s = b[i];
        for k in 0..i {
            s -= a[i * n + k] * b[k];
        }
        b[i] = s / a[i * n + i];
    }
    for i in (0..n).rev() {
        let mut s = b[i];
        for k in i + 1..n {
            s -= a[k * n + i] * b[k];
        }
        b[i] = s / a[i * n + i];
    }
    Ok(())
}

// ===================================================================================================
// Önjáték
// ===================================================================================================

pub struct TrainOptions {
    pub games: u64,
    pub jobs: usize,
    pub seed_start: u64,
    /// a lépés ekkora eséllyel az első öt legjobb közül véletlenszerű (a rossz maradékokról is legyen adat)
    pub epsilon: f64,
    pub lambda: f64,
    /// csak azok a lerakások adnak mintát, amelyeknél a zsákban legalább ennyi zseton volt a lépés előtt
    pub min_bag: usize,
    /// minden ennyiedik játék a kivárt (validációs) halmazba kerül (0 = nincs): a λ kiválasztásához
    pub holdout_every: u64,
}

pub struct TrainReport {
    pub leaves: LinearLeaves,
    pub games: u64,
    pub samples: u64,
    pub mean_y: f64,
    pub sd_y: f64,
    /// a tanító és a kivárt halmaz összegzői (újraoldáshoz, mentéshez)
    pub train: Normal,
    pub validation: Option<Normal>,
}

impl std::fmt::Debug for TrainReport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "TrainReport {{ games: {}, samples: {} }}", self.games, self.samples)
    }
}

type Policy<'a> = &'a (dyn Fn(&EvalContext<'_>, &Play) -> f64 + Sync);

/// Egy önjáték-játék mintái a `normal`-ba. A játékosok a megadott értékelővel játszanak.
pub fn play_training_game(
    config: &GameConfig,
    lexicon: &Lexicon,
    policy: Policy<'_>,
    seed: u64,
    epsilon: f64,
    min_bag: usize,
    normal: &mut Normal,
) -> Result<(), String> {
    let mut game = Game::new(config.clone(), &["A", "B"], seed);
    let mut generator = MoveGenerator::new(config);
    let mut rng = Rng::seed_from_u64(mix(seed ^ 0x7121_1A1D));
    struct Sample {
        who: usize,
        feats: Vec<(usize, f64)>,
        spread_after: i32,
    }
    let mut samples: Vec<Sample> = Vec::with_capacity(32);
    let mut guard = 0;
    while !game.is_over() {
        guard += 1;
        if guard > 400 {
            return Err(format!("a(z) {seed}. magú tanítójáték nem ért véget"));
        }
        let who = game.to_move();
        let rack = *game.current_rack();
        let bag = game.bag().len();
        let spread = game.players()[who].score - game.players()[1 - who].score;
        let plays: Vec<Play> = game.legal_plays(&mut generator, lexicon).to_vec();
        if plays.is_empty() {
            let turn = if bag >= config.min_exchange_bag { Turn::Exchange(rack) } else { Turn::Pass };
            game.apply(turn, lexicon).map_err(|e| format!("tanítójáték, csere / passz: {e:?}"))?;
            continue;
        }
        let ctx = EvalContext { config, board: game.board(), lexicon, rack: &rack, bag_remaining: bag, spread };
        let mut scored: Vec<(Play, f64)> = plays.iter().map(|p| (*p, policy(&ctx, p))).collect();
        scored.sort_by(|a, b| b.1.total_cmp(&a.1));
        let pick = if epsilon > 0.0 && rng.next_f64() < epsilon { rng.below(scored.len().min(5) as u64) as usize } else { 0 };
        let play = scored[pick].0;
        let leave = play.leave(&rack);
        game.apply(Turn::Place(play), lexicon).map_err(|e| format!("tanítójáték, lerakás: {e:?}"))?;
        if bag >= min_bag
            && let Some(leave) = leave
        {
            let mut feats = Vec::with_capacity(24);
            features(&leave, &mut feats);
            samples.push(Sample { who, feats, spread_after: game.players()[who].score - game.players()[1 - who].score });
        }
    }
    let scores = game.scores();
    for s in samples {
        let final_spread = scores[s.who] - scores[1 - s.who];
        normal.add(&s.feats, (final_spread - s.spread_after) as f64);
    }
    Ok(())
}

/// Önjáték-tanítás `opts.jobs` szálon. A magok `seed_start`-tól folynak (a kiértékelő magok fölött).
pub fn train(config: &GameConfig, lexicon: &Lexicon, policy: Policy<'_>, opts: &TrainOptions, progress: bool) -> Result<TrainReport, String> {
    if opts.seed_start < TRAIN_SEED_START {
        return Err(format!("a tanító magoknak legalább {TRAIN_SEED_START}-nak kell lenniük (a kiértékelő magoktól elkülönítve)"));
    }
    let next = AtomicU64::new(0);
    let failure = std::sync::Mutex::new(None::<String>);
    let merged = std::sync::Mutex::new((Normal::new(), Normal::new()));
    std::thread::scope(|scope| {
        for _ in 0..opts.jobs.max(1) {
            scope.spawn(|| {
                let (mut local, mut local_val) = (Normal::new(), Normal::new());
                loop {
                    let i = next.fetch_add(1, Ordering::SeqCst);
                    if i >= opts.games || failure.lock().unwrap().is_some() {
                        break;
                    }
                    let held_out = opts.holdout_every > 0 && i % opts.holdout_every == opts.holdout_every - 1;
                    let target = if held_out { &mut local_val } else { &mut local };
                    if let Err(e) = play_training_game(config, lexicon, policy, opts.seed_start + i, opts.epsilon, opts.min_bag, target) {
                        *failure.lock().unwrap() = Some(e);
                        break;
                    }
                    if progress && (i + 1).is_multiple_of(20_000) {
                        eprintln!("  tanítás: {} / {} játék", i + 1, opts.games);
                    }
                }
                let mut m = merged.lock().unwrap();
                m.0.merge(&local);
                m.1.merge(&local_val);
            });
        }
    });
    if let Some(e) = failure.into_inner().unwrap() {
        return Err(e);
    }
    let (train, validation) = merged.into_inner().unwrap();
    let leaves = train.solve(opts.lambda)?;
    Ok(TrainReport {
        leaves,
        games: opts.games,
        samples: train.samples + validation.samples,
        mean_y: train.mean_y(),
        sd_y: train.sd_y(),
        validation: if validation.samples > 0 { Some(validation) } else { None },
        train,
    })
}

/// A tanítás magjainak származtatása (tesztekhez): minden játék külön, a kiértékelő magoktól elkülönülő mag.
pub fn training_seed(index: u64) -> u64 {
    TRAIN_SEED_START + derive(index, 1, 2) % 1_000_000_000
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rack_of(tiles: &[u8]) -> Rack {
        Rack::from_tiles(tiles.iter().map(|&t| if t == 255 { pg_scrabble::tile::Tile::BLANK } else { pg_scrabble::tile::Tile::letter(t) }))
    }

    #[test]
    fn features_count_tiles_and_pairs() {
        let mut f = Vec::new();
        features(&rack_of(&[0, 0, 5]), &mut f);
        // A×2 (tile 0 → 2), 5-ös betű ×1, pár (0,0) = 1, pár (0,5) = 2·1, nincs (5,5) pár
        let get = |i: usize| f.iter().find(|x| x.0 == i).map(|x| x.1);
        assert_eq!(get(0), Some(2.0));
        assert_eq!(get(5), Some(1.0));
        assert_eq!(get(pair_index(0, 0)), Some(1.0));
        assert_eq!(get(pair_index(0, 5)), Some(2.0));
        assert_eq!(get(pair_index(5, 5)), None);
        assert!(f.windows(2).all(|w| w[0].0 < w[1].0), "növekvő index");
    }

    #[test]
    fn the_blank_has_its_own_slot() {
        let mut f = Vec::new();
        features(&rack_of(&[255, 3]), &mut f);
        assert!(f.iter().any(|x| x.0 == BLANK_SLOT && x.1 == 1.0));
        assert!(f.iter().any(|x| x.0 == pair_index(3, BLANK_SLOT)));
    }

    #[test]
    fn the_empty_leave_is_worth_nothing() {
        let leaves = LinearLeaves { weights: (0..N_FEATURES).map(|i| i as f64 * 0.01).collect() };
        assert_eq!(leaves.value(&Rack::new()), 0.0);
    }

    #[test]
    fn the_solver_recovers_known_weights() {
        // szintetikus adat: y = 3 + Σ w·x + zaj; a tengelymetszet kiesik, a súlyok visszajönnek
        let mut truth = vec![0.0; N_FEATURES];
        truth[0] = 5.0;
        truth[1] = -4.0;
        truth[pair_index(0, 1)] = 2.5;
        truth[BLANK_SLOT] = 25.0;
        let model = LinearLeaves { weights: truth.clone() };
        let mut normal = Normal::new();
        let mut rng = Rng::seed_from_u64(7);
        for _ in 0..60_000 {
            let n = 1 + rng.below(6) as usize;
            let tiles: Vec<u8> = (0..n).map(|_| if rng.below(30) == 0 { 255 } else { rng.below(6) as u8 }).collect();
            let rack = rack_of(&tiles);
            let mut f = Vec::new();
            features(&rack, &mut f);
            let noise = (rng.next_f64() - 0.5) * 20.0;
            normal.add(&f, 3.0 + model.value(&rack) + noise);
        }
        let fit = normal.solve(1e-6).unwrap();
        for i in [0usize, 1, pair_index(0, 1), BLANK_SLOT] {
            assert!((fit.weights[i] - truth[i]).abs() < 0.6, "weight {i}: {} vs {}", fit.weights[i], truth[i]);
        }
        assert!(fit.weights[2].abs() < 0.6, "egy nem használt jellemző nullához közeli");
    }

    #[test]
    fn the_normal_equation_survives_a_file_and_measures_the_error() {
        let model = LinearLeaves { weights: (0..N_FEATURES).map(|i| if i < 6 { i as f64 } else { 0.0 }).collect() };
        let mut normal = Normal::new();
        let mut rng = Rng::seed_from_u64(11);
        for _ in 0..5_000 {
            let tiles: Vec<u8> = (0..1 + rng.below(5)).map(|_| rng.below(6) as u8).collect();
            let rack = rack_of(&tiles);
            let mut f = Vec::new();
            features(&rack, &mut f);
            normal.add(&f, 2.0 + model.value(&rack));
        }
        let path = std::env::temp_dir().join(format!("normal-test-{}.bin", std::process::id()));
        normal.save(&path).unwrap();
        let back = Normal::load(&path).unwrap();
        assert_eq!(back.samples, normal.samples);
        let fit = back.solve_full(1e-6).unwrap();
        assert!(back.mse(&fit) < 1e-6, "pontos adatnál a hiba ~0: {}", back.mse(&fit));
        // egy rosszabb modell hibája nagyobb
        let mut worse = fit.clone();
        worse.weights[0] += 3.0;
        assert!(back.mse(&worse) > back.mse(&fit) + 0.1);
        assert!((fit.weights[BIAS] - 2.0).abs() < 1e-3, "a tengelymetszet: {}", fit.weights[BIAS]);
        std::fs::write(&path, b"nem normalegyenlet").unwrap();
        assert!(Normal::load(&path).is_err());
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn json_round_trip_and_validation() {
        let leaves = LinearLeaves { weights: (0..N_FEATURES).map(|i| (i as f64).sin()).collect() };
        let value = leaves.to_json(json!({"games": 1}));
        let back = LinearLeaves::from_json(&value).unwrap();
        assert_eq!(back.weights, leaves.weights);
        assert!(LinearLeaves::from_json(&json!({"weights": [1.0, 2.0]})).is_err());
    }

    #[test]
    fn the_training_seeds_stay_away_from_the_evaluation_seeds() {
        assert!((0..1000).all(|i| training_seed(i) >= TRAIN_SEED_START));
    }
}
