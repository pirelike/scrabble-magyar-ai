//! A JSONL naplók feldolgozása és a jelentés: a mérőszámok mind **párszintűek** (a tükrözött pár a mintaegység).

use super::stats::{self, Summary};
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::PathBuf;

#[derive(Clone, Debug, Default)]
pub struct SideStats {
    pub plays: u64,
    pub exch: u64,
    pub pass: u64,
    pub bingos: u64,
    pub points: i64,
    pub cpu_ms: f64,
    pub eg_calls: u64,
    pub eg_exact: u64,
}

#[derive(Clone, Debug)]
pub struct Rec {
    pub pair: u64,
    pub half: u8,
    pub score_a: i32,
    pub score_b: i32,
    pub played_out: bool,
    pub plies: u64,
    pub a: SideStats,
    pub b: SideStats,
    pub spread_at_bag_empty: Option<i32>,
}

impl Rec {
    pub fn spread(&self) -> i32 {
        self.score_a - self.score_b
    }

    /// Az `a` oldal győzelmi pontja: 1 / ½ / 0.
    pub fn win_a(&self) -> f64 {
        match self.score_a.cmp(&self.score_b) {
            std::cmp::Ordering::Greater => 1.0,
            std::cmp::Ordering::Equal => 0.5,
            std::cmp::Ordering::Less => 0.0,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Matchup {
    pub spec_a: String,
    pub spec_b: String,
    pub recs: Vec<Rec>,
}

fn side_of(v: &Value) -> SideStats {
    SideStats {
        plays: v["plays"].as_u64().unwrap_or(0),
        exch: v["exch"].as_u64().unwrap_or(0),
        pass: v["pass"].as_u64().unwrap_or(0),
        bingos: v["bingos"].as_u64().unwrap_or(0),
        points: v["points"].as_i64().unwrap_or(0),
        cpu_ms: v["cpu_ms"].as_f64().unwrap_or(0.0),
        eg_calls: v["eg_calls"].as_u64().unwrap_or(0),
        eg_exact: v["eg_exact"].as_u64().unwrap_or(0),
    }
}

pub fn parse_line(line: &str) -> Result<(String, String, Rec), String> {
    let v: Value = serde_json::from_str(line).map_err(|e| format!("hibás naplósor: {e}"))?;
    let field = |k: &str| v[k].as_str().map(|s| s.to_string()).ok_or_else(|| format!("hiányzó mező: {k}"));
    Ok((
        field("spec_a")?,
        field("spec_b")?,
        Rec {
            pair: v["pair"].as_u64().ok_or("hiányzó pár")?,
            half: v["half"].as_u64().ok_or("hiányzó fél")? as u8,
            score_a: v["score_a"].as_i64().ok_or("hiányzó pont")? as i32,
            score_b: v["score_b"].as_i64().ok_or("hiányzó pont")? as i32,
            played_out: v["end"] == "out",
            plies: v["plies"].as_u64().unwrap_or(0),
            a: side_of(&v["side_a"]),
            b: side_of(&v["side_b"]),
            spread_at_bag_empty: v["spread_at_bag_empty"].as_i64().map(|x| x as i32),
        },
    ))
}

pub fn load(paths: &[PathBuf]) -> Result<Vec<Matchup>, String> {
    let mut by: BTreeMap<(String, String), Vec<Rec>> = BTreeMap::new();
    for path in paths {
        let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        for line in text.lines().filter(|l| !l.trim().is_empty()) {
            let (a, b, rec) = parse_line(line)?;
            by.entry((a, b)).or_default().push(rec);
        }
    }
    Ok(by.into_iter().map(|((spec_a, spec_b), recs)| Matchup { spec_a, spec_b, recs }).collect())
}

#[derive(Clone, Debug)]
pub struct Analysis {
    pub spec_a: String,
    pub spec_b: String,
    pub pairs: usize,
    /// a pár-átlag pontkülönbségek (A − B)
    pub pair_spread: Vec<f64>,
    pub spread: Summary,
    pub spread_boot: (f64, f64),
    pub win: Summary,
    pub win_wilson: (f64, f64),
    pub sign: (u64, u64, u64, f64),
    /// az A hány győzelmi pontot szerzett a párban (0, ½, 1, 1½, 2): darabszámok
    pub outcomes: [u64; 5],
    pub mean_score_a: f64,
    pub mean_score_b: f64,
    pub points_per_turn: (f64, f64),
    pub plays_per_game: (f64, f64),
    pub bingos_per_game: (f64, f64),
    pub exch_per_game: (f64, f64),
    pub cpu_ms_per_game: (f64, f64),
    pub endgame_swing: Option<(f64, usize)>,
    pub scoreless_share: f64,
    pub plies: f64,
    pub endgame_exact: ((u64, u64), (u64, u64)),
    pub duplicate_games: usize,
}

pub fn analyze(m: &Matchup) -> Option<Analysis> {
    let mut halves: BTreeMap<u64, [Option<&Rec>; 2]> = BTreeMap::new();
    let mut duplicate_games = 0;
    for r in &m.recs {
        let slot = &mut halves.entry(r.pair).or_insert([None, None])[(r.half & 1) as usize];
        if slot.is_some() {
            duplicate_games += 1;
        } else {
            *slot = Some(r);
        }
    }
    let complete: Vec<(&Rec, &Rec)> = halves.values().filter_map(|h| Some((h[0]?, h[1]?))).collect();
    if complete.is_empty() {
        return None;
    }
    let games: Vec<&Rec> = complete.iter().flat_map(|(x, y)| [*x, *y]).collect();
    let g = games.len() as f64;
    let pair_spread: Vec<f64> = complete.iter().map(|(x, y)| (x.spread() + y.spread()) as f64 / 2.0).collect();
    let pair_win: Vec<f64> = complete.iter().map(|(x, y)| (x.win_a() + y.win_a()) / 2.0).collect();
    let spread = stats::summarize(&pair_spread);
    let spread_boot = stats::bootstrap_ci(&pair_spread, 10_000, 0xD0E1);
    let win = stats::summarize(&pair_win);
    let game_wins: f64 = games.iter().map(|r| r.win_a()).sum();
    let win_wilson = stats::wilson(game_wins, g);
    let (mut w, mut l, mut d) = (0u64, 0u64, 0u64);
    for s in &pair_spread {
        if *s > 0.0 {
            w += 1;
        } else if *s < 0.0 {
            l += 1;
        } else {
            d += 1;
        }
    }
    let mut outcomes = [0u64; 5];
    for (x, y) in &complete {
        outcomes[((x.win_a() + y.win_a()) * 2.0).round() as usize] += 1;
    }
    let mean = |f: &dyn Fn(&Rec) -> f64| games.iter().map(|r| f(r)).sum::<f64>() / g;
    let per_turn = |side: &dyn Fn(&Rec) -> &SideStats| {
        let points: i64 = games.iter().map(|r| side(r).points).sum();
        let turns: u64 = games.iter().map(|r| side(r).plays + side(r).exch + side(r).pass).sum();
        points as f64 / turns.max(1) as f64
    };
    let swings: Vec<f64> = games.iter().filter_map(|r| r.spread_at_bag_empty.map(|s| (r.spread() - s) as f64)).collect();
    let endgame_swing = if swings.is_empty() { None } else { Some((swings.iter().sum::<f64>() / swings.len() as f64, swings.len())) };
    let eg = |side: &dyn Fn(&Rec) -> &SideStats| (games.iter().map(|r| side(r).eg_calls).sum::<u64>(), games.iter().map(|r| side(r).eg_exact).sum::<u64>());
    Some(Analysis {
        spec_a: m.spec_a.clone(),
        spec_b: m.spec_b.clone(),
        pairs: complete.len(),
        pair_spread,
        spread,
        spread_boot,
        win,
        win_wilson,
        sign: (w, l, d, stats::sign_test_p(w, l)),
        outcomes,
        mean_score_a: mean(&|r| r.score_a as f64),
        mean_score_b: mean(&|r| r.score_b as f64),
        points_per_turn: (per_turn(&|r| &r.a), per_turn(&|r| &r.b)),
        plays_per_game: (mean(&|r| r.a.plays as f64), mean(&|r| r.b.plays as f64)),
        bingos_per_game: (mean(&|r| r.a.bingos as f64), mean(&|r| r.b.bingos as f64)),
        exch_per_game: (mean(&|r| r.a.exch as f64), mean(&|r| r.b.exch as f64)),
        cpu_ms_per_game: (mean(&|r| r.a.cpu_ms), mean(&|r| r.b.cpu_ms)),
        endgame_swing,
        scoreless_share: games.iter().filter(|r| !r.played_out).count() as f64 / g,
        plies: mean(&|r| r.plies as f64),
        endgame_exact: (eg(&|r| &r.a), eg(&|r| &r.b)),
        duplicate_games,
    })
}

fn signed(x: f64) -> String {
    format!("{x:+.1}")
}

/// Markdown jelentés (magyarul): összefoglaló táblázat + részletek minden párosításról.
pub fn render(list: &[Analysis]) -> String {
    let mut out = String::new();
    out.push_str("| A | B | Párok | Átlagos különbség (A − B) | SE | 95% CI (t) | 95% CI (bootstrap) | p | A nyerési aránya |\n|---|---|---:|---:|---:|---|---|---:|---:|\n");
    for a in list {
        out.push_str(&format!(
            "| `{}` | `{}` | {} | **{}** | {:.2} | [{}, {}] | [{}, {}] | {} | {:.1}% |\n",
            a.spec_a,
            a.spec_b,
            a.pairs,
            signed(a.spread.mean),
            a.spread.se,
            signed(a.spread.ci95.0),
            signed(a.spread.ci95.1),
            signed(a.spread_boot.0),
            signed(a.spread_boot.1),
            if a.spread.p < 0.001 { "<0,001".to_string() } else { format!("{:.3}", a.spread.p).replace('.', ",") },
            a.win.mean * 100.0
        ));
    }
    for a in list {
        out.push_str(&format!("\n#### `{}` — `{}` ({} pár, {} játék)\n\n", a.spec_a, a.spec_b, a.pairs, a.pairs * 2));
        out.push_str("| Mutató | A | B |\n|---|---:|---:|\n");
        out.push_str(&format!("| Átlagos pontszám játékonként | {:.1} | {:.1} |\n", a.mean_score_a, a.mean_score_b));
        out.push_str(&format!("| Pont egy körre (lerakás, csere, passz együtt) | {:.2} | {:.2} |\n", a.points_per_turn.0, a.points_per_turn.1));
        out.push_str(&format!("| Lerakások száma játékonként | {:.1} | {:.1} |\n", a.plays_per_game.0, a.plays_per_game.1));
        out.push_str(&format!("| Bingó játékonként | {:.2} | {:.2} |\n", a.bingos_per_game.0, a.bingos_per_game.1));
        out.push_str(&format!("| Csere játékonként | {:.2} | {:.2} |\n", a.exch_per_game.0, a.exch_per_game.1));
        out.push_str(&format!("| Processzoridő játékonként (ms) | {:.0} | {:.0} |\n", a.cpu_ms_per_game.0, a.cpu_ms_per_game.1));
        out.push_str(&format!(
            "| Végjáték-kereső hívások (pontos / összes) | {} / {} | {} / {} |\n",
            a.endgame_exact.0.1, a.endgame_exact.0.0, a.endgame_exact.1.1, a.endgame_exact.1.0
        ));
        out.push_str("\n| Mutató | Érték |\n|---|---|\n");
        out.push_str(&format!(
            "| Átlagos különbség (A − B), páronként | {} ± {:.2} (SE), a pár-átlagok szórása {:.1} |\n",
            signed(a.spread.mean),
            a.spread.se,
            a.spread.sd
        ));
        out.push_str(&format!(
            "| A győzelmi aránya (döntetlen = ½) | {:.1}% (párszintű SE ±{:.1}%; játékszintű Wilson-intervallum {:.1}–{:.1}%) |\n",
            a.win.mean * 100.0,
            a.win.se * 100.0,
            a.win_wilson.0 * 100.0,
            a.win_wilson.1 * 100.0
        ));
        out.push_str(&format!("| Előjelpróba (párok): A nyer / B nyer / döntetlen | {} / {} / {}, p = {:.3} |\n", a.sign.0, a.sign.1, a.sign.2, a.sign.3));
        out.push_str(&format!(
            "| A győzelmi pontjai a párban (0 / ½ / 1 / 1½ / 2) | {} / {} / {} / {} / {} pár |\n",
            a.outcomes[0], a.outcomes[1], a.outcomes[2], a.outcomes[3], a.outcomes[4]
        ));
        if let Some((swing, n)) = a.endgame_swing {
            out.push_str(&format!("| Végjáték-hozam (A különbsége a zsák kiürülésétől a végéig), {n} játékban | {} |\n", signed(swing)));
        }
        out.push_str(&format!(
            "| Kirakással végződő játékok aránya | {:.1}% (a többi 6 pont nélküli körrel ért véget); átlagosan {:.1} lépés |\n",
            (1.0 - a.scoreless_share) * 100.0,
            a.plies
        ));
        if a.duplicate_games > 0 {
            out.push_str(&format!("| Figyelmeztetés | {} duplikált játéksor kihagyva |\n", a.duplicate_games));
        }
    }
    out
}
