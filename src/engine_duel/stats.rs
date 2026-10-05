//! Statisztika a párszintű elemzéshez: középérték, Student-féle konfidenciaintervallum és p-érték, bootstrap, Wilson-
//! intervallum, előjelpróba. Külső csomag nélkül (a tesztek ismert értékekkel vetik össze).

use pg_scrabble::rng::Rng;

/// ln Γ(x), Lanczos-közelítés.
fn ln_gamma(x: f64) -> f64 {
    const C: [f64; 6] = [76.18009172947146, -86.50532032941677, 24.01409824083091, -1.231739572450155, 0.1208650973866179e-2, -0.5395239384953e-5];
    let mut y = x;
    let tmp = x + 5.5;
    let tmp = tmp - (x + 0.5) * tmp.ln();
    let mut ser = 1.000000000190015;
    for c in C {
        y += 1.0;
        ser += c / y;
    }
    -tmp + (2.5066282746310005 * ser / x).ln()
}

/// A regularizált nem teljes béta-függvény I_x(a, b) (Numerical Recipes, folytonos tört).
fn inc_beta(a: f64, b: f64, x: f64) -> f64 {
    if x <= 0.0 {
        return 0.0;
    }
    if x >= 1.0 {
        return 1.0;
    }
    let bt = (ln_gamma(a + b) - ln_gamma(a) - ln_gamma(b) + a * x.ln() + b * (1.0 - x).ln()).exp();
    if x < (a + 1.0) / (a + b + 2.0) { bt * beta_cf(a, b, x) / a } else { 1.0 - bt * beta_cf(b, a, 1.0 - x) / b }
}

fn beta_cf(a: f64, b: f64, x: f64) -> f64 {
    const TINY: f64 = 1e-300;
    let (qab, qap, qam) = (a + b, a + 1.0, a - 1.0);
    let mut c = 1.0;
    let mut d = 1.0 - qab * x / qap;
    if d.abs() < TINY {
        d = TINY;
    }
    d = 1.0 / d;
    let mut h = d;
    for m in 1..300 {
        let m = m as f64;
        let m2 = 2.0 * m;
        let aa = m * (b - m) * x / ((qam + m2) * (a + m2));
        d = 1.0 + aa * d;
        if d.abs() < TINY {
            d = TINY;
        }
        c = 1.0 + aa / c;
        if c.abs() < TINY {
            c = TINY;
        }
        d = 1.0 / d;
        h *= d * c;
        let aa = -(a + m) * (qab + m) * x / ((a + m2) * (qap + m2));
        d = 1.0 + aa * d;
        if d.abs() < TINY {
            d = TINY;
        }
        c = 1.0 + aa / c;
        if c.abs() < TINY {
            c = TINY;
        }
        d = 1.0 / d;
        let del = d * c;
        h *= del;
        if (del - 1.0).abs() < 3e-16 {
            break;
        }
    }
    h
}

/// Kétoldali p-érték Student-féle t-eloszlásból (`df` szabadságfok).
pub fn t_two_sided_p(t: f64, df: f64) -> f64 {
    if !t.is_finite() {
        return 0.0;
    }
    inc_beta(df / 2.0, 0.5, df / (df + t * t))
}

/// A kétoldali 95%-os intervallumhoz tartozó t-kvantilis (a p = 0,05 gyöke, felezéssel).
pub fn t_quantile_975(df: f64) -> f64 {
    let (mut lo, mut hi) = (0.0f64, 200.0f64);
    for _ in 0..200 {
        let mid = (lo + hi) / 2.0;
        if t_two_sided_p(mid, df) > 0.05 {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    (lo + hi) / 2.0
}

#[derive(Clone, Copy, Debug)]
pub struct Summary {
    pub n: usize,
    pub mean: f64,
    pub sd: f64,
    pub se: f64,
    pub ci95: (f64, f64),
    /// az „átlag = 0” nullhipotézis kétoldali p-értéke (Student)
    pub p: f64,
}

pub fn summarize(xs: &[f64]) -> Summary {
    let n = xs.len();
    if n == 0 {
        return Summary { n: 0, mean: f64::NAN, sd: f64::NAN, se: f64::NAN, ci95: (f64::NAN, f64::NAN), p: f64::NAN };
    }
    let mean = xs.iter().sum::<f64>() / n as f64;
    if n < 2 {
        return Summary { n, mean, sd: f64::NAN, se: f64::NAN, ci95: (f64::NAN, f64::NAN), p: f64::NAN };
    }
    let var = xs.iter().map(|x| (x - mean) * (x - mean)).sum::<f64>() / (n as f64 - 1.0);
    let sd = var.sqrt();
    let se = sd / (n as f64).sqrt();
    let q = t_quantile_975(n as f64 - 1.0);
    let p = if se == 0.0 { if mean == 0.0 { 1.0 } else { 0.0 } } else { t_two_sided_p(mean / se, n as f64 - 1.0) };
    Summary { n, mean, sd, se, ci95: (mean - q * se, mean + q * se), p }
}

/// Percentilis-bootstrap 95%-os intervallum az átlagra (a párok az egységek).
pub fn bootstrap_ci(xs: &[f64], resamples: usize, seed: u64) -> (f64, f64) {
    if xs.is_empty() {
        return (f64::NAN, f64::NAN);
    }
    let mut rng = Rng::seed_from_u64(seed);
    let n = xs.len();
    let mut means: Vec<f64> = (0..resamples)
        .map(|_| {
            let mut sum = 0.0;
            for _ in 0..n {
                sum += xs[rng.below(n as u64) as usize];
            }
            sum / n as f64
        })
        .collect();
    means.sort_by(|a, b| a.total_cmp(b));
    let at = |q: f64| means[((q * (resamples - 1) as f64).round() as usize).min(resamples - 1)];
    (at(0.025), at(0.975))
}

/// Wilson-intervallum (95%) egy aránynak: `k` siker `n`-ből.
pub fn wilson(k: f64, n: f64) -> (f64, f64) {
    if n <= 0.0 {
        return (f64::NAN, f64::NAN);
    }
    let z = 1.959964;
    let p = k / n;
    let denom = 1.0 + z * z / n;
    let center = (p + z * z / (2.0 * n)) / denom;
    let half = z * (p * (1.0 - p) / n + z * z / (4.0 * n * n)).sqrt() / denom;
    (center - half, center + half)
}

/// Egzakt kétoldali előjelpróba (nulla-esélyű pár nem számít): p-érték `wins` / `losses` mellett.
pub fn sign_test_p(wins: u64, losses: u64) -> f64 {
    let n = wins + losses;
    if n == 0 {
        return 1.0;
    }
    let k = wins.min(losses);
    // P(X <= k) binomiális(n, 1/2), kétszerezve
    let mut tail = 0.0;
    for i in 0..=k {
        tail += (ln_choose(n, i) - n as f64 * std::f64::consts::LN_2).exp();
    }
    (2.0 * tail).min(1.0)
}

fn ln_choose(n: u64, k: u64) -> f64 {
    ln_gamma(n as f64 + 1.0) - ln_gamma(k as f64 + 1.0) - ln_gamma((n - k) as f64 + 1.0)
}

/// Hány pár kell ahhoz, hogy az átlag szórása (`se`) a célérték alá menjen, ha a pár-átlagok szórása `sd`.
pub fn pairs_needed(sd: f64, target_se: f64) -> u64 {
    ((sd / target_se).powi(2)).ceil() as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn student_quantiles_match_the_tables() {
        for (df, expected) in [(1.0, 12.706), (5.0, 2.571), (10.0, 2.228), (30.0, 2.042), (120.0, 1.980)] {
            let q = t_quantile_975(df);
            assert!((q - expected).abs() < 0.002, "df {df}: {q} vs {expected}");
        }
        assert!((t_quantile_975(1e6) - 1.96).abs() < 0.001);
    }

    #[test]
    fn p_values_are_sane() {
        assert!((t_two_sided_p(2.228, 10.0) - 0.05).abs() < 0.001);
        assert!((t_two_sided_p(0.0, 20.0) - 1.0).abs() < 1e-9);
        assert!(t_two_sided_p(10.0, 50.0) < 1e-10);
    }

    #[test]
    fn the_summary_of_a_known_sample() {
        let s = summarize(&[1.0, 2.0, 3.0, 4.0, 5.0]);
        assert_eq!(s.n, 5);
        assert!((s.mean - 3.0).abs() < 1e-12);
        assert!((s.sd - 1.5811388).abs() < 1e-6);
        assert!((s.se - std::f64::consts::FRAC_1_SQRT_2).abs() < 1e-6);
        // t(0.975, 4) = 2.776
        assert!((s.ci95.1 - (3.0 + 2.776 * std::f64::consts::FRAC_1_SQRT_2)).abs() < 0.01);
        assert!((s.p - 0.0 - t_two_sided_p(3.0 / std::f64::consts::FRAC_1_SQRT_2, 4.0)).abs() < 1e-9);
        let z = summarize(&[0.0, 0.0, 0.0]);
        assert_eq!(z.p, 1.0);
    }

    #[test]
    fn the_bootstrap_interval_brackets_the_mean() {
        let mut rng = Rng::seed_from_u64(3);
        let xs: Vec<f64> = (0..400).map(|_| 10.0 + (rng.next_f64() - 0.5) * 40.0).collect();
        let s = summarize(&xs);
        let (lo, hi) = bootstrap_ci(&xs, 2000, 9);
        assert!(lo < s.mean && s.mean < hi);
        assert!((hi - lo - 2.0 * 1.96 * s.se).abs() < 0.4 * s.se * 2.0 * 1.96);
        assert_eq!(bootstrap_ci(&xs, 500, 9), bootstrap_ci(&xs, 500, 9), "megismételhető");
    }

    #[test]
    fn wilson_and_sign_test() {
        let (lo, hi) = wilson(50.0, 100.0);
        assert!((lo - 0.4038).abs() < 0.001 && (hi - 0.5962).abs() < 0.001, "{lo} {hi}");
        assert!((sign_test_p(7, 3) - 0.34375).abs() < 1e-9);
        assert!((sign_test_p(0, 10) - 0.001953125).abs() < 1e-9);
        assert_eq!(sign_test_p(5, 5), 1.0);
        assert_eq!(sign_test_p(0, 0), 1.0);
    }

    #[test]
    fn the_number_of_pairs_needed() {
        assert_eq!(pairs_needed(60.0, 3.0), 400);
        assert_eq!(pairs_needed(40.0, 3.0), 178);
    }
}
