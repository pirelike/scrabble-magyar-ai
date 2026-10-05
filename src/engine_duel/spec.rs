//! Az oldalak szöveges leírása és a futtatási környezet (szótár, szabályok, tanult maradék-értékek).
//!
//! ```text
//! bot:10            a robot 10. fokozata, a szókincsére korlátozva (a motor ugyanezt a szótárt látja)
//! bot:9, bot:8      alacsonyabb fokozatok (kontroll)
//! bot:10:full       a robot az éles működése szerint (a keresztszavak a teljes szótárból is jöhetnek)
//! bot:greedy        csak a pontszám számít (kontroll)
//! eng:greedy        a motor, csak a pontszám
//! eng:stock         a motor gyári értékelése (magyar ábécén gyakorlatilag csak a joker és az ismétlés)
//! eng:leaves        a motor a tanult maradék-értékekkel (`--leaves FÁJL`)
//! eng:leaves:sim-fast   + Monte-Carlo szimuláció (`sim-fast` | `sim` | `sim-deep`)
//! eng:leaves:sim-fast+eg  + pontos végjáték-kereső (`+eg` vagy `+eg=CSOMÓPONT`)
//! ```

use super::leaves::LinearLeaves;
use super::lexicon::{build_lexicon, engine_config};
use super::sides::{BotMode, BotSide, DEFAULT_ENDGAME_BUDGET, EngineSide, Eval, Search, Side};
use crate::ai::{self, Vocabulary};
use pg_scrabble::eval::StaticEvaluator;
use pg_scrabble::prelude::*;
use pg_scrabble::sim::SimOptions;
use std::path::Path;
use std::sync::Arc;

pub struct Env {
    pub vocab: Arc<Vocabulary>,
    pub config: Arc<GameConfig>,
    pub lexicon: Arc<Lexicon>,
    pub leaves: Option<Arc<LinearLeaves>>,
    /// a motor szótárának zsetonsorai (a naplóhoz)
    pub sequences: usize,
}

impl Env {
    /// Betölti a szótárat és a robot szókincsét (a szótárnak elérhetőnek kell lennie: különben a játékvezető mindent
    /// érvényesnek venne, és az összevetés értelmetlen lenne).
    pub fn load(leaves: Option<&Path>) -> Result<Env, String> {
        let leaves = match leaves {
            Some(path) => Some(LinearLeaves::load(path)?),
            None => None,
        };
        Env::load_with(leaves)
    }

    /// Mint a `load`, de a tanult maradék-értékek már a memóriában vannak (tesztekhez).
    pub fn load_with(leaves: Option<LinearLeaves>) -> Result<Env, String> {
        if !crate::dictionary::is_available() {
            return Err("a szótár nem tölthető be (a dict/ mappa hiányzik?): a játékvezető enélkül nem megbízható".to_string());
        }
        let vocab = ai::get_vocabulary();
        if vocab.is_empty() {
            return Err("a robot szókincse üres".to_string());
        }
        let config = engine_config();
        let (lexicon, sequences) = build_lexicon(&config, &vocab)?;
        Ok(Env { vocab, config: Arc::new(config), lexicon: Arc::new(lexicon), leaves: leaves.map(Arc::new), sequences })
    }

    pub fn build_side(&self, spec: &str) -> Result<Box<dyn Side>, String> {
        let (body, endgame) = match spec.split_once("+eg") {
            Some((body, rest)) => {
                let budget = match rest.strip_prefix('=') {
                    Some(n) => n.parse::<u64>().map_err(|_| format!("hibás végjáték-keret: {spec}"))?,
                    None if rest.is_empty() => DEFAULT_ENDGAME_BUDGET,
                    None => return Err(format!("ismeretlen oldal: {spec}")),
                };
                (body, Some(budget))
            }
            None => (spec, None),
        };
        let parts: Vec<&str> = body.split(':').collect();
        match parts.as_slice() {
            ["bot", "greedy"] if endgame.is_none() => Ok(Box::new(BotSide::new(ai::MAX_LEVEL, BotMode::Greedy, self.vocab.clone()))),
            ["bot", level] if endgame.is_none() => Ok(Box::new(BotSide::new(parse_level(level, spec)?, BotMode::Strict, self.vocab.clone()))),
            ["bot", level, "full"] if endgame.is_none() => Ok(Box::new(BotSide::new(parse_level(level, spec)?, BotMode::Production, self.vocab.clone()))),
            ["eng", eval] => self.engine(spec, eval, None, endgame),
            ["eng", eval, search] => self.engine(spec, eval, Some(search), endgame),
            _ => Err(format!("ismeretlen oldal: {spec} (lásd a src/engine_duel/spec.rs fejlécét)")),
        }
    }

    fn engine(&self, spec: &str, eval: &str, search: Option<&str>, endgame: Option<u64>) -> Result<Box<dyn Side>, String> {
        let evaluator = match eval {
            "greedy" => Eval::Greedy,
            "stock" => Eval::Stock(StaticEvaluator::new()),
            "leaves" => match &self.leaves {
                Some(leaves) => Eval::Leaves((**leaves).clone()),
                None => return Err(format!("{spec}: a tanult maradék-értékekhez kell a --leaves FÁJL")),
            },
            other => return Err(format!("ismeretlen értékelés: {other}")),
        };
        let search = match search {
            None => Search::Static,
            Some("sim-fast") => Search::Sim(SimOptions::fast()),
            Some("sim") => Search::Sim(SimOptions::default()),
            Some("sim-deep") => Search::Sim(SimOptions::deep()),
            Some(other) => return Err(format!("ismeretlen keresés: {other}")),
        };
        Ok(Box::new(EngineSide::new(spec.to_string(), self.config.clone(), self.lexicon.clone(), Arc::new(evaluator), search, endgame)))
    }
}

fn parse_level(text: &str, spec: &str) -> Result<u8, String> {
    match text.parse::<u8>() {
        Ok(l) if (ai::MIN_LEVEL..=ai::MAX_LEVEL).contains(&l) => Ok(l),
        _ => Err(format!("hibás fokozat: {spec}")),
    }
}
