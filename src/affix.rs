//! Beépített, külső függőség nélküli Hunspell-szerű szóellenőrző a `dict/hu_HU.{aff,dic}` fájlokhoz.
//!
//! Támogatott: szótő, egy előtag, legfeljebb két (egymásra épülő) végződés, a folytatási osztályok (az `AF`
//! aliasok), NEEDAFFIX / ONLYINCOMPOUND / FORBIDDENWORD jelzők. Szándékosan NINCS összetételi (COMPOUND*)
//! támogatás: a Hunspell összetétel-szabályai tetszőleges betűsorokat is elfogadnak, a Scrabble-ban viszont
//! csak a szótárban szereplő szavak és azok ragozott alakjai érvényesek.
//!
//! A szavakat kisbetűvel kell keresni (a nagy kezdőbetűs tulajdonnevek így nem érvényesek).
//!
//! A szótár (helyesírás-ellenőrzésre készült) minden nyelvtanilag lehetséges alakot elfogad, a Scrabble-ban
//! viszont ez sok értelmetlen szót engedne. Ezért a szótár morfológiai címkéi (`AM` aliasok: szófaj, a
//! toldalék fajtája) alapján:
//! - a csak kötőjellel toldalékolható szócikkek (rövidítések, mértékegységek, idegen írásmódú szavak)
//!   nem érvényesek;
//! - a „kockázatos” levezetések — melléknév birtokos személyjellel (KEDVESEM, de TAROM), -ék családi többes
//!   (SZOMSZÉDÉK, de ÉJÉK), -ul/-ül főnéven (FELESÉGÜL, de BLÖKIÜL) és a -né képző (KIRÁLYNÉ, de ALMÁNÉ) —
//!   csak akkor érvényesek, ha az alak a használatban ténylegesen előfordul (`attested`).
//!
//! A fájlok bájtokként olvasódnak: a jelzők egyetlen (nyers) bájtok, az `.aff` fejléce vegyes kódolású.

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::Arc;

const MAX_AFFIX_LEN: usize = 40;

// A szabályok kockázati jelzői (a morfológiai címkéjükből)
const RISK_FAMILIAR: u8 = 1; // -ék családi többes
const RISK_ESSIVE: u8 = 2; // -ul/-ül: főnéven kockázatos, melléknéven szabályos
const RISK_POSS: u8 = 4; // birtokos személyjel / birtokjel: melléknéven kockázatos
const RISK_DERIVED: u8 = 8; // ugyanabban a szabályban képző + az előbbiek kockázatos párosítása
const RISK_DERIVATION: u8 = 16; // -né képző
const RISK_ALWAYS: u8 = RISK_FAMILIAR | RISK_DERIVED | RISK_DERIVATION;
const RISK_NAMES: [(u8, &str); 5] = [
    (RISK_FAMILIAR, "familiar"),
    (RISK_ESSIVE, "essive"),
    (RISK_POSS, "possessive"),
    (RISK_DERIVED, "derived"),
    (RISK_DERIVATION, "derivation"),
];

const RISKY_DERIVATIONS: [&[u8]; 1] = [b"ds:n\xc3\xa9_MRS_"];
const ADJ_LIKE_DERIVATIONS: [&[u8]; 1] = [b"ds:s_OCCUPATION_"];
const NOUN_LIKE_DERIVATIONS: [&[u8]; 2] = [b"ds:\xc3\x93_PRESPART_", b"ds:And\xc3\x93_FUTPART_"];

/// A szócikkek fajtája (a morfológiai leírásukból)
pub const KIND_ADJ: u8 = 1; // melléknév (po:adj)
pub const KIND_FOREIGN: u8 = 2; // csak kötőjellel toldalékolható főnév (al:szó-)

fn is_ws(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c)
}

/// A Python `re.split(rb'[ \t]+', line.strip(b' \t\r'))`.
fn split_ws(line: &[u8]) -> Vec<&[u8]> {
    let mut start = 0;
    let mut end = line.len();
    while start < end && matches!(line[start], b' ' | b'\t' | b'\r') {
        start += 1;
    }
    while end > start && matches!(line[end - 1], b' ' | b'\t' | b'\r') {
        end -= 1;
    }
    let line = &line[start..end];
    let mut parts = Vec::new();
    let mut i = 0;
    let mut part_start = 0;
    while i < line.len() {
        if matches!(line[i], b' ' | b'\t') {
            parts.push(&line[part_start..i]);
            while i < line.len() && matches!(line[i], b' ' | b'\t') {
                i += 1;
            }
            part_start = i;
        } else {
            i += 1;
        }
    }
    parts.push(&line[part_start..]);
    parts
}

fn split_ascii_ws(text: &[u8]) -> Vec<&[u8]> {
    text.split(|b| is_ws(*b)).filter(|t| !t.is_empty()).collect()
}

fn contains_subslice(haystack: &[u8], needle: &[u8]) -> bool {
    needle.is_empty() || haystack.windows(needle.len()).any(|w| w == needle)
}

fn is_digits(token: &[u8]) -> bool {
    !token.is_empty() && token.iter().all(|b| b.is_ascii_digit())
}

fn parse_usize(token: &[u8]) -> Option<usize> {
    std::str::from_utf8(token).ok()?.parse().ok()
}

// ---------- a szabályok feltételei (a Hunspell `[^aeiou]` stílusú, egyszerű reguláris kifejezései) ----------

#[derive(Clone, Debug)]
enum ClassItem {
    Char(char),
    Range(char, char),
}

#[derive(Clone, Debug)]
enum CondElem {
    Any,
    Lit(char),
    Class { negated: bool, items: Vec<ClassItem> },
}

impl CondElem {
    fn matches(&self, c: char) -> bool {
        match self {
            CondElem::Any => c != '\n',
            CondElem::Lit(l) => *l == c,
            CondElem::Class { negated, items } => {
                let found = items.iter().any(|item| match item {
                    ClassItem::Char(x) => *x == c,
                    ClassItem::Range(a, b) => *a <= c && c <= *b,
                });
                found != *negated
            }
        }
    }
}

/// Egy szabály feltétele: rögzített hosszú elemsor, amely a szótő végére (SFX) vagy elejére (PFX) illeszkedik.
#[derive(Clone, Debug)]
struct Cond {
    elems: Vec<CondElem>,
}

impl Cond {
    /// Hibás kifejezésnél (pl. fordított tartomány: `[ű-à]`) None — a Python `re.compile` is hibát jelezne,
    /// és a szabályt eldobnánk.
    fn parse(source: &str) -> Option<Cond> {
        let chars: Vec<char> = source.chars().collect();
        let mut elems = Vec::new();
        let mut i = 0;
        while i < chars.len() {
            let c = chars[i];
            if c == '[' {
                i += 1;
                let negated = i < chars.len() && chars[i] == '^';
                if negated {
                    i += 1;
                }
                let mut items = Vec::new();
                let mut first = true;
                loop {
                    if i >= chars.len() {
                        return None; // lezáratlan osztály
                    }
                    let ch = chars[i];
                    if ch == ']' && !first {
                        i += 1;
                        break;
                    }
                    first = false;
                    // tartomány: x-y, ha a '-' után nem ']' áll
                    if i + 2 < chars.len() && chars[i + 1] == '-' && chars[i + 2] != ']' {
                        let (a, b) = (ch, chars[i + 2]);
                        if b < a {
                            return None; // fordított tartomány
                        }
                        items.push(ClassItem::Range(a, b));
                        i += 3;
                    } else {
                        items.push(ClassItem::Char(ch));
                        i += 1;
                    }
                }
                elems.push(CondElem::Class { negated, items });
            } else if c == '.' {
                elems.push(CondElem::Any);
                i += 1;
            } else {
                elems.push(CondElem::Lit(c));
                i += 1;
            }
        }
        Some(Cond { elems })
    }

    fn matches_end(&self, word: &str) -> bool {
        let n = self.elems.len();
        let chars: Vec<char> = word.chars().rev().take(n).collect();
        if chars.len() < n {
            return false;
        }
        self.elems.iter().rev().zip(chars.iter()).all(|(e, c)| e.matches(*c))
    }

    fn matches_start(&self, word: &str) -> bool {
        let n = self.elems.len();
        let chars: Vec<char> = word.chars().take(n).collect();
        if chars.len() < n {
            return false;
        }
        self.elems.iter().zip(chars.iter()).all(|(e, c)| e.matches(*c))
    }
}

// ---------- szabályok és szócikkek ----------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Derived {
    Noun,
    Adj,
    Other,
}

impl Derived {
    pub fn as_str(self) -> &'static str {
        match self {
            Derived::Noun => "noun",
            Derived::Adj => "adj",
            Derived::Other => "other",
        }
    }
}

/// (kockázati jelzők, képzett szófaj) egy szabály morfológiai leírásából.
fn rule_risk(morph: &[u8]) -> (u8, Option<Derived>) {
    if morph.is_empty() {
        return (0, None);
    }
    let tokens = split_ascii_ws(morph);
    let mut derived: Option<Derived> = None;
    for token in &tokens {
        if ADJ_LIKE_DERIVATIONS.iter().any(|p| token.starts_with(p)) {
            derived = Some(Derived::Adj);
        } else if NOUN_LIKE_DERIVATIONS.iter().any(|p| token.starts_with(p)) {
            derived = Some(Derived::Noun);
        } else if token.starts_with(b"ds:") {
            derived = Some(if token.ends_with(b"_noun") {
                Derived::Noun
            } else if token.ends_with(b"_adj") {
                Derived::Adj
            } else {
                Derived::Other
            });
        } else if token.starts_with(b"is:") && (token.ends_with(b"_adj") || token.ends_with(b"_vrb")) {
            // a szótár néhány képzőt is:-sel jelöl (-i, -bb, -bbik, -nyi, -jú, -hat)
            derived = Some(if token.ends_with(b"_adj") { Derived::Adj } else { Derived::Other });
        }
    }
    let mut risk = 0;
    if tokens.iter().any(|t| t.starts_with(b"is:") && contains_subslice(t, b"_FAMILIAR")) {
        risk |= RISK_FAMILIAR;
    }
    if tokens.iter().any(|t| *t == b"is:ESS") {
        match derived {
            Some(Derived::Noun) => risk |= RISK_DERIVED,
            None => risk |= RISK_ESSIVE,
            _ => {}
        }
    }
    if tokens.iter().any(|t| RISKY_DERIVATIONS.iter().any(|p| t.starts_with(p))) {
        risk |= RISK_DERIVATION;
    }
    if tokens.iter().any(|t| t.starts_with(b"is:POSS")) {
        match derived {
            Some(Derived::Adj) => risk |= RISK_DERIVED,
            None => risk |= RISK_POSS,
            _ => {}
        }
    }
    (risk, derived)
}

/// Egy ragozási szabály (PFX vagy SFX sor).
#[derive(Debug)]
pub struct Rule {
    pub flag: u8,
    cross: bool,
    pub strip: String,
    pub add: String,
    cont: Option<Arc<[u8]>>,
    cond: Option<Cond>,
    need_affix: bool,
    only_in_compound: bool,
    risk: u8,
    derived: Option<Derived>,
}

impl Rule {
    fn cont_has(&self, flag: u8) -> bool {
        self.cont.as_ref().is_some_and(|c| c.contains(&flag))
    }

    fn cond_ok_suffix(&self, stem: &str) -> bool {
        self.cond.as_ref().is_none_or(|c| c.matches_end(stem))
    }

    fn cond_ok_prefix(&self, stem: &str) -> bool {
        self.cond.as_ref().is_none_or(|c| c.matches_start(stem))
    }
}

/// Kockázatos-e a (`kind` fajtájú) szótő + `rule` (+ `outer`) levezetés?
fn is_risky(kind: u8, rule: &Rule, outer: Option<&Rule>) -> bool {
    if rule.risk & RISK_ALWAYS != 0 {
        return true;
    }
    let adj = kind & KIND_ADJ != 0;
    if (rule.risk & RISK_POSS != 0 && adj) || (rule.risk & RISK_ESSIVE != 0 && !adj) {
        return true;
    }
    if let Some(outer) = outer {
        if outer.risk & RISK_ALWAYS != 0 {
            return true;
        }
        // a belső szabály után a szófaj: a képzett, vagy (ha nem képző) a szótőé
        let inner_adj = rule.derived == Some(Derived::Adj) || (rule.derived.is_none() && adj);
        if (outer.risk & RISK_POSS != 0 && inner_adj) || (outer.risk & RISK_ESSIVE != 0 && !inner_adj) {
            return true;
        }
    }
    false
}

#[derive(Clone, Debug)]
struct Entry {
    flags: Arc<[u8]>,
    kind: u8,
}

/// Egy levezetés a szó-vizsgáló eszköznek.
#[derive(Clone, Debug, PartialEq)]
pub struct Explanation {
    pub stem: String,
    pub prefix: Option<String>,
    pub suffixes: Vec<String>,
    pub risky: bool,
    pub risk: Vec<&'static str>,
    pub adjective: bool,
    pub derived: Option<&'static str>,
}

pub struct AffixChecker {
    needaffix: Option<u8>,
    onlyincompound: Option<u8>,
    forbidden: Option<u8>,
    prefixes: HashMap<String, Vec<Rule>>,
    suffixes: HashMap<String, Vec<Rule>>,
    cont_flags: [bool; 256],
    entries: HashMap<String, Vec<Entry>>,
    attested: Option<HashSet<String>>,
}

struct Loader {
    aliases: Vec<Arc<[u8]>>,
    aliases_defined: bool,
    morph_aliases: Vec<Vec<u8>>,
    morph_aliases_defined: bool,
}

impl Loader {
    /// Folytatási / szótári jelzők: az `AF` alias sorszáma (vagy közvetlen jelzők, ha nincs alias).
    fn alias(&self, token: &[u8]) -> Option<Arc<[u8]>> {
        if token.is_empty() {
            return None;
        }
        if self.aliases_defined && is_digits(token) {
            let index = parse_usize(token)?;
            if index > 0 && index < self.aliases.len() {
                return Some(self.aliases[index].clone());
            }
            return None;
        }
        Some(Arc::from(token))
    }

    /// Morfológiai leírás: az `AM` alias sorszáma (vagy maga a leírás, ha nincs alias).
    fn morph(&self, field: &[u8]) -> Vec<u8> {
        let field = trim_ws(field);
        if field.is_empty() {
            return Vec::new();
        }
        if self.morph_aliases_defined && is_digits(field) {
            return match parse_usize(field) {
                Some(index) if index > 0 && index < self.morph_aliases.len() => self.morph_aliases[index].clone(),
                _ => Vec::new(),
            };
        }
        field.to_vec()
    }
}

fn trim_ws(text: &[u8]) -> &[u8] {
    let mut start = 0;
    let mut end = text.len();
    while start < end && is_ws(text[start]) {
        start += 1;
    }
    while end > start && is_ws(text[end - 1]) {
        end -= 1;
    }
    &text[start..end]
}

fn has_pos(morph: &[u8], pos: &[u8]) -> bool {
    split_ascii_ws(morph).iter().any(|t| t.len() == pos.len() + 3 && t.starts_with(b"po:") && &t[3..] == pos)
}

fn entry_kind(word: &[u8], morph: &[u8]) -> u8 {
    let mut kind = 0;
    if has_pos(morph, b"adj") {
        kind |= KIND_ADJ;
    }
    let mut needle = b"al:".to_vec();
    needle.extend_from_slice(word);
    needle.push(b'-');
    if contains_subslice(morph, &needle) && has_pos(morph, b"noun") {
        kind |= KIND_FOREIGN;
    }
    kind
}

fn lossy(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

impl AffixChecker {
    /// Betöltés. `attested_path`: a ténylegesen használt „kockázatos” alakok listája (soronként egy szó, `#`
    /// megjegyzés); None esetén a kockázatos levezetések is érvényesek (a hunspell viselkedése).
    pub fn load(aff_path: &Path, dic_path: &Path, attested_path: Option<&Path>) -> std::io::Result<AffixChecker> {
        let aff = std::fs::read(aff_path)?;
        let dic = std::fs::read(dic_path)?;
        let attested = match attested_path {
            Some(path) => Some(load_attested(path)?),
            None => None,
        };
        Ok(AffixChecker::from_bytes(&aff, &dic, attested))
    }

    pub fn from_bytes(aff: &[u8], dic: &[u8], attested: Option<HashSet<String>>) -> AffixChecker {
        let lines: Vec<&[u8]> = aff.split(|b| *b == b'\n').collect();
        let mut loader = Loader {
            aliases: vec![Arc::from(&b""[..])],
            aliases_defined: lines.iter().any(|l| l.starts_with(b"AF ")),
            morph_aliases: vec![Vec::new()],
            morph_aliases_defined: lines.iter().any(|l| l.starts_with(b"AM ")),
        };
        let mut checker = AffixChecker {
            needaffix: None,
            onlyincompound: None,
            forbidden: None,
            prefixes: HashMap::new(),
            suffixes: HashMap::new(),
            cont_flags: [false; 256],
            entries: HashMap::new(),
            attested,
        };
        checker.load_aff(&lines, &mut loader);
        checker.load_dic(dic, &loader);
        checker
    }

    fn load_aff(&mut self, lines: &[&[u8]], loader: &mut Loader) {
        let mut header_seen = false;
        let mut morph_header_seen = false;
        let mut rules: Vec<(bool, Vec<&[u8]>)> = Vec::new();
        for line in lines {
            if line.is_empty() || line[0] == b'#' {
                continue;
            }
            let parts = split_ws(line);
            let key = parts[0];
            match key {
                b"AF" => {
                    if !header_seen {
                        header_seen = true; // az első AF sor a darabszám
                        continue;
                    }
                    let value: &[u8] = if parts.len() > 1 { parts[1] } else { b"" };
                    loader.aliases.push(Arc::from(value));
                }
                b"AM" => {
                    if !morph_header_seen {
                        morph_header_seen = true; // az első AM sor a darabszám
                        continue;
                    }
                    let stripped = trim_chars(line, b" \t\r");
                    loader.morph_aliases.push(trim_ws(&stripped[2.min(stripped.len())..]).to_vec());
                }
                b"NEEDAFFIX" => self.needaffix = parts.get(1).and_then(|p| p.first().copied()),
                b"ONLYROOT" if self.needaffix.is_none() => {
                    self.needaffix = parts.get(1).and_then(|p| p.first().copied())
                }
                b"ONLYINCOMPOUND" => self.onlyincompound = parts.get(1).and_then(|p| p.first().copied()),
                b"FORBIDDENWORD" => self.forbidden = parts.get(1).and_then(|p| p.first().copied()),
                b"PFX" => rules.push((true, parts)),
                b"SFX" => rules.push((false, parts)),
                _ => {}
            }
        }

        let mut pfx_cross: HashMap<u8, bool> = HashMap::new();
        let mut sfx_cross: HashMap<u8, bool> = HashMap::new();
        for (is_prefix, parts) in rules {
            let Some(flag) = parts.get(1).and_then(|p| p.first().copied()) else { continue };
            if parts.len() == 4 && (parts[2] == b"Y" || parts[2] == b"N") && is_digits(parts[3]) {
                let table = if is_prefix { &mut pfx_cross } else { &mut sfx_cross };
                table.insert(flag, parts[2] == b"Y");
                continue;
            }
            if parts.len() < 4 {
                continue;
            }
            let cross = if is_prefix { pfx_cross.get(&flag) } else { sfx_cross.get(&flag) }.copied().unwrap_or(false);
            self.add_rule(is_prefix, flag, cross, &parts, loader);
        }
    }

    fn add_rule(&mut self, is_prefix: bool, flag: u8, cross: bool, parts: &[&[u8]], loader: &Loader) {
        let mut strip = lossy(parts[2]);
        let (add_field, cont_field) = match parts[3].iter().position(|b| *b == b'/') {
            Some(i) => (&parts[3][..i], &parts[3][i + 1..]),
            None => (parts[3], &b""[..]),
        };
        let mut add = lossy(add_field);
        if strip == "0" {
            strip.clear();
        }
        if add == "0" {
            add.clear();
        }
        let cont = if cont_field.is_empty() { None } else { loader.alias(cont_field) };
        let condition = if parts.len() > 4 { lossy(parts[4]) } else { ".".to_string() };
        let cond = if condition != "." {
            match Cond::parse(&condition) {
                Some(cond) => Some(cond),
                None => return, // hibás feltétel: a szabályt eldobjuk
            }
        } else {
            None
        };
        let cont = cont.filter(|c| !c.is_empty());
        let need_affix = match (&cont, self.needaffix) {
            (Some(c), Some(f)) => c.contains(&f),
            _ => false,
        };
        let only_in_compound = match (&cont, self.onlyincompound) {
            (Some(c), Some(f)) => c.contains(&f),
            _ => false,
        };
        let (risk, derived) = if is_prefix {
            (0, None)
        } else {
            let mut joined = Vec::new();
            for (i, p) in parts[5.min(parts.len())..].iter().enumerate() {
                if i > 0 {
                    joined.push(b' ');
                }
                joined.extend_from_slice(p);
            }
            rule_risk(&loader.morph(&joined))
        };
        if let Some(c) = &cont {
            for b in c.iter() {
                self.cont_flags[*b as usize] = true;
            }
        }
        let rule = Rule { flag, cross, strip, add: add.clone(), cont, cond, need_affix, only_in_compound, risk, derived };
        let table = if is_prefix { &mut self.prefixes } else { &mut self.suffixes };
        table.entry(add).or_default().push(rule);
    }

    fn load_dic(&mut self, dic: &[u8], loader: &Loader) {
        let mut first = true;
        for raw in dic.split(|b| *b == b'\n') {
            if first {
                first = false;
                if is_digits(trim_ws(raw)) {
                    continue;
                }
            }
            let mut line = raw;
            while let Some((last, rest)) = line.split_last() {
                if *last == b'\r' || *last == b'\n' {
                    line = rest;
                } else {
                    break;
                }
            }
            if line.is_empty() {
                continue;
            }
            let (entry, morph_field) = match line.iter().position(|b| *b == b'\t') {
                Some(i) => (&line[..i], &line[i + 1..]),
                None => (line, &b""[..]),
            };
            let (word, flags) = match entry.iter().position(|b| *b == b'/') {
                Some(i) => (&entry[..i], &entry[i + 1..]),
                None => (entry, &b""[..]),
            };
            if word.contains(&b' ') {
                continue; // többszavas tétel
            }
            let flags = match flags.iter().position(|b| *b == b' ') {
                Some(i) => &flags[..i],
                None => flags,
            };
            let flag_set: Arc<[u8]> = if flags.is_empty() {
                Arc::from(&b""[..])
            } else {
                loader.alias(flags).unwrap_or_else(|| Arc::from(&b""[..]))
            };
            let kind = if morph_field.is_empty() { 0 } else { entry_kind(word, &loader.morph(morph_field)) };
            self.entries.entry(lossy(word)).or_default().push(Entry { flags: flag_set, kind });
        }
    }

    pub fn entry_count(&self) -> usize {
        self.entries.len()
    }

    /// A szótári szócikkek szavai (a szótő-listákhoz).
    pub fn entry_words(&self) -> impl Iterator<Item = &String> {
        self.entries.keys()
    }

    pub fn has_entry(&self, word: &str) -> bool {
        self.entries.contains_key(word)
    }

    // ---------- keresés ----------

    /// A tiltott, csak összetételben álló és a csak kötőjellel toldalékolható szócikkek nélkül.
    fn usable_entries<'a>(&'a self, word: &str) -> impl Iterator<Item = &'a Entry> + 'a {
        let forbidden = self.forbidden;
        let onlyincompound = self.onlyincompound;
        self.entries.get(word).into_iter().flatten().filter(move |e| {
            if e.kind & KIND_FOREIGN != 0 {
                return false;
            }
            if forbidden.is_some_and(|f| e.flags.contains(&f)) {
                return false;
            }
            if onlyincompound.is_some_and(|f| e.flags.contains(&f)) {
                return false;
            }
            true
        })
    }

    fn stem_ok(
        &self,
        stem: &str,
        rule: &Rule,
        pfx: Option<&Rule>,
        outer_flag: Option<u8>,
        outer_rule: Option<&Rule>,
        allow_risky: bool,
    ) -> bool {
        for entry in self.usable_entries(stem) {
            if !entry.flags.contains(&rule.flag) && !pfx.is_some_and(|p| p.cont_has(rule.flag)) {
                continue;
            }
            if let Some(p) = pfx {
                if !entry.flags.contains(&p.flag) && !rule.cont_has(p.flag) {
                    continue;
                }
            }
            if let Some(of) = outer_flag {
                if !rule.cont_has(of) {
                    continue;
                }
            }
            if !allow_risky && is_risky(entry.kind, rule, outer_rule) {
                continue;
            }
            return true;
        }
        false
    }

    /// Azok az SFX szabálycsoportok, amelyek `add` része a szó vége, és marad szótő.
    fn suffix_candidates<'a>(&'a self, word: &str) -> Vec<(&'a str, &'a Vec<Rule>)> {
        let boundaries = char_boundaries(word);
        let n = boundaries.len() - 1;
        let mut found = Vec::new();
        for k in 0..=(n.saturating_sub(1)).min(MAX_AFFIX_LEN) {
            let add = &word[boundaries[n - k]..];
            if let Some((key, rules)) = self.suffixes.get_key_value(add) {
                found.push((key.as_str(), rules));
            }
        }
        found
    }

    fn prefix_candidates<'a>(&'a self, word: &str) -> Vec<(&'a str, &'a Vec<Rule>)> {
        let boundaries = char_boundaries(word);
        let n = boundaries.len() - 1;
        let mut found = Vec::new();
        for k in 0..=(n.saturating_sub(1)).min(MAX_AFFIX_LEN) {
            let add = &word[..boundaries[k]];
            if let Some((key, rules)) = self.prefixes.get_key_value(add) {
                found.push((key.as_str(), rules));
            }
        }
        found
    }

    /// Szótő + egy végződés (a `pfx` előtaggal keresztezve, ha meg van adva).
    fn suffix_check(
        &self,
        word: &str,
        pfx: Option<&Rule>,
        outer_flag: Option<u8>,
        outer_rule: Option<&Rule>,
        allow_risky: bool,
    ) -> bool {
        for (add, rules) in self.suffix_candidates(word) {
            let base = &word[..word.len() - add.len()];
            for rule in rules {
                if outer_flag.is_some() && rule.cont.is_none() {
                    continue;
                }
                if rule.only_in_compound {
                    continue;
                }
                if outer_flag.is_none() && rule.need_affix && !pfx.is_some_and(|p| !p.need_affix) {
                    continue;
                }
                if let Some(p) = pfx {
                    if !(rule.cross && p.cross) {
                        continue;
                    }
                }
                let stem = format!("{base}{}", rule.strip);
                if !rule.cond_ok_suffix(&stem) {
                    continue;
                }
                if self.stem_ok(&stem, rule, pfx, outer_flag, outer_rule, allow_risky) {
                    return true;
                }
            }
        }
        false
    }

    /// Szótő + két egymásra épülő végződés.
    fn suffix_check_two(&self, word: &str, pfx: Option<&Rule>, allow_risky: bool) -> bool {
        for (add, rules) in self.suffix_candidates(word) {
            let base = &word[..word.len() - add.len()];
            for rule in rules {
                if !self.cont_flags[rule.flag as usize] || rule.only_in_compound {
                    continue;
                }
                if let Some(p) = pfx {
                    if !(rule.cross && p.cross) {
                        continue;
                    }
                }
                let stem = format!("{base}{}", rule.strip);
                if !rule.cond_ok_suffix(&stem) {
                    continue;
                }
                let inner_pfx = match pfx {
                    Some(p) if !rule.cont_has(p.flag) => Some(p),
                    _ => None,
                };
                if self.suffix_check(&stem, inner_pfx, Some(rule.flag), Some(rule), allow_risky) {
                    return true;
                }
            }
        }
        false
    }

    fn prefix_check(&self, word: &str, two_suffixes: bool, allow_risky: bool) -> bool {
        for (add, rules) in self.prefix_candidates(word) {
            let rest = &word[add.len()..];
            for rule in rules {
                if rule.only_in_compound {
                    continue;
                }
                let stem = format!("{}{rest}", rule.strip);
                if !rule.cond_ok_prefix(&stem) {
                    continue;
                }
                if !two_suffixes {
                    if self.usable_entries(&stem).any(|e| e.flags.contains(&rule.flag)) {
                        return true;
                    }
                    if rule.cross && self.suffix_check(&stem, Some(rule), None, None, allow_risky) {
                        return true;
                    }
                } else if rule.cross && self.suffix_check_two(&stem, Some(rule), allow_risky) {
                    return true;
                }
            }
        }
        false
    }

    fn is_forbidden(&self, word: &str) -> bool {
        match (self.entries.get(word), self.forbidden) {
            (Some(entries), Some(f)) => entries.iter().any(|e| e.flags.contains(&f)),
            _ => false,
        }
    }

    fn compute(&self, word: &str, allow_risky: bool) -> bool {
        if self.is_forbidden(word) {
            return false;
        }
        if self.has_stem(word) {
            return true;
        }
        if self.prefix_check(word, false, allow_risky) {
            return true;
        }
        if self.suffix_check(word, None, None, None, allow_risky) {
            return true;
        }
        if self.suffix_check_two(word, None, allow_risky) {
            return true;
        }
        self.prefix_check(word, true, allow_risky)
    }

    fn risky_allowed(&self, word: &str) -> bool {
        self.attested.as_ref().is_none_or(|a| a.contains(word))
    }

    /// Igaz, ha a (kisbetűs) szó a szótárban szerepel vagy annak ragozott alakja. A kockázatos levezetés
    /// csak a ténylegesen használt alakoknál számít.
    pub fn check(&self, word: &str) -> bool {
        self.compute(word, self.risky_allowed(word))
    }

    /// Igaz, ha a szó önmagában (toldalék nélkül) érvényes szótári tétel.
    pub fn has_stem(&self, word: &str) -> bool {
        if self.is_forbidden(word) {
            return false;
        }
        self.usable_entries(word).any(|e| self.needaffix.is_none_or(|f| !e.flags.contains(&f)))
    }

    /// Igaz, ha a szó csak kockázatos levezetéssel érvényes (a használati listán múlik).
    pub fn needs_attestation(&self, word: &str) -> bool {
        !self.compute(word, false) && self.compute(word, true)
    }

    pub fn is_attested(&self, word: &str) -> bool {
        self.attested.as_ref().is_some_and(|a| a.contains(word))
    }

    pub fn has_attested_list(&self) -> bool {
        self.attested.is_some()
    }

    // ---------- levezetések (a szó-vizsgáló eszköznek) ----------

    fn matching_kind(&self, stem: &str, rule: &Rule, pfx: Option<&Rule>, outer_flag: Option<u8>) -> Option<u8> {
        for entry in self.usable_entries(stem) {
            if !entry.flags.contains(&rule.flag) && !pfx.is_some_and(|p| p.cont_has(rule.flag)) {
                continue;
            }
            if let Some(p) = pfx {
                if !entry.flags.contains(&p.flag) && !rule.cont_has(p.flag) {
                    continue;
                }
            }
            if let Some(of) = outer_flag {
                if !rule.cont_has(of) {
                    continue;
                }
            }
            return Some(entry.kind);
        }
        None
    }

    /// A `suffix_check` találatai: (szabály, szótő, szócikk-fajta).
    fn suffix_hits<'a>(&'a self, word: &str, pfx: Option<&Rule>, outer_flag: Option<u8>) -> Vec<(&'a Rule, String, u8)> {
        let mut hits = Vec::new();
        for (add, rules) in self.suffix_candidates(word) {
            let base = &word[..word.len() - add.len()];
            for rule in rules {
                if outer_flag.is_some() && rule.cont.is_none() {
                    continue;
                }
                if rule.only_in_compound {
                    continue;
                }
                if outer_flag.is_none() && rule.need_affix && !pfx.is_some_and(|p| !p.need_affix) {
                    continue;
                }
                if let Some(p) = pfx {
                    if !(rule.cross && p.cross) {
                        continue;
                    }
                }
                let stem = format!("{base}{}", rule.strip);
                if !rule.cond_ok_suffix(&stem) {
                    continue;
                }
                if let Some(kind) = self.matching_kind(&stem, rule, pfx, outer_flag) {
                    hits.push((rule, stem, kind));
                }
            }
        }
        hits
    }

    fn risk_names(rule: Option<&Rule>) -> Vec<&'static str> {
        match rule {
            Some(rule) => RISK_NAMES.iter().filter(|(bit, _)| rule.risk & bit != 0).map(|(_, name)| *name).collect(),
            None => Vec::new(),
        }
    }

    /// A (kisbetűs) szó levezetései a szótár szabályaiból. A tiltott / csak összetételben álló szócikkek nélkül.
    /// Nem a döntéshez való (azt a `check` adja), hanem a szó-vizsgáló eszköz magyarázatához.
    pub fn explain(&self, word: &str, limit: usize) -> Vec<Explanation> {
        let mut found: Vec<Explanation> = Vec::new();

        let add = |found: &mut Vec<Explanation>,
                       stem: &str,
                       prefix: Option<&str>,
                       suffixes: Vec<String>,
                       rule: Option<&Rule>,
                       kind: u8,
                       outer: Option<&Rule>| {
            if found.len() >= limit {
                return;
            }
            let risky = rule.is_some_and(|r| is_risky(kind, r, outer));
            let mut risk = Self::risk_names(rule);
            if outer.is_some() {
                risk.extend(Self::risk_names(outer));
            }
            risk.sort();
            risk.dedup();
            let entry = Explanation {
                stem: stem.to_string(),
                prefix: prefix.map(|p| p.to_string()),
                suffixes,
                risky,
                risk,
                adjective: kind & KIND_ADJ != 0,
                derived: rule.and_then(|r| r.derived).map(|d| d.as_str()),
            };
            if !found.contains(&entry) {
                found.push(entry);
            }
        };

        if self.has_stem(word) {
            add(&mut found, word, None, Vec::new(), None, 0, None);
        }
        for (add_text, prules) in self.prefix_candidates(word) {
            let rest = &word[add_text.len()..];
            for prule in prules {
                if prule.only_in_compound {
                    continue;
                }
                let stem = format!("{}{rest}", prule.strip);
                if !prule.cond_ok_prefix(&stem) {
                    continue;
                }
                if let Some(entry) = self.usable_entries(&stem).find(|e| e.flags.contains(&prule.flag)) {
                    add(&mut found, &stem, Some(add_text), Vec::new(), Some(prule), entry.kind, None);
                }
                if prule.cross {
                    for (srule, sstem, kind) in self.suffix_hits(&stem, Some(prule), None) {
                        add(&mut found, &sstem, Some(add_text), vec![srule.add.clone()], Some(srule), kind, None);
                    }
                }
            }
        }
        for (srule, stem, kind) in self.suffix_hits(word, None, None) {
            add(&mut found, &stem, None, vec![srule.add.clone()], Some(srule), kind, None);
        }
        for (add_text, orules) in self.suffix_candidates(word) {
            let base = &word[..word.len() - add_text.len()];
            for orule in orules {
                if !self.cont_flags[orule.flag as usize] || orule.only_in_compound {
                    continue;
                }
                let inner = format!("{base}{}", orule.strip);
                if !orule.cond_ok_suffix(&inner) {
                    continue;
                }
                for (irule, stem, kind) in self.suffix_hits(&inner, None, Some(orule.flag)) {
                    add(&mut found, &stem, None, vec![irule.add.clone(), orule.add.clone()], Some(irule), kind, Some(orule));
                }
            }
        }
        found
    }

    // ---------- ragozott alakok előállítása ----------

    /// A megadott szótövek ragozott alakjai: szótő + EGY végződés, a szótár saját szabályaival.
    ///
    /// `adds`: a megengedett végződések — a teljes szabályrendszer több tízmillió alakot adna, ezért csak a
    /// kért végződésekkel dolgozunk. A tővégi a/e nyúlásával járó alak (alma → almá-ban: a szabály `add`
    /// része 'ában') ugyanannak a végződésnek számít. A szótári szavakat nem adja vissza újra. A kockázatos
    /// levezetésből csak a ténylegesen használt alakokat adja — `risky` hamis értékénél egyet sem.
    pub fn inflected_forms(&self, stems: &[String], adds: &HashSet<&str>, max_form_len: usize, risky: bool) -> HashSet<String> {
        let mut by_flag: HashMap<u8, Vec<&Rule>> = HashMap::new();
        for rules in self.suffixes.values() {
            for rule in rules {
                if rule.need_affix || rule.only_in_compound {
                    continue;
                }
                let wanted = adds.contains(rule.add.as_str()) || {
                    let mut chars = rule.add.chars();
                    match chars.next() {
                        Some(first) if (rule.strip == "a" || rule.strip == "e") && (first == 'á' || first == 'é') => {
                            adds.contains(chars.as_str())
                        }
                        _ => false,
                    }
                };
                if wanted {
                    by_flag.entry(rule.flag).or_default().push(rule);
                }
            }
        }
        let mut forms = HashSet::new();
        for stem in stems {
            for entry in self.usable_entries(stem) {
                for flag in entry.flags.iter() {
                    let Some(rules) = by_flag.get(flag) else { continue };
                    for rule in rules {
                        if !rule.strip.is_empty() && !stem.ends_with(&rule.strip) {
                            continue;
                        }
                        if !rule.cond_ok_suffix(stem) {
                            continue;
                        }
                        let base = &stem[..stem.len() - rule.strip.len()];
                        let form = format!("{base}{}", rule.add);
                        if form == *stem || form.chars().count() > max_form_len || self.entries.contains_key(&form) {
                            continue;
                        }
                        if is_risky(entry.kind, rule, None) && !(risky && self.risky_allowed(&form)) {
                            continue;
                        }
                        forms.insert(form);
                    }
                }
            }
        }
        forms
    }
}

fn char_boundaries(word: &str) -> Vec<usize> {
    let mut boundaries: Vec<usize> = word.char_indices().map(|(i, _)| i).collect();
    boundaries.push(word.len());
    boundaries
}

fn trim_chars<'a>(text: &'a [u8], chars: &[u8]) -> &'a [u8] {
    let mut start = 0;
    let mut end = text.len();
    while start < end && chars.contains(&text[start]) {
        start += 1;
    }
    while end > start && chars.contains(&text[end - 1]) {
        end -= 1;
    }
    &text[start..end]
}

pub fn load_attested(path: &Path) -> std::io::Result<HashSet<String>> {
    let text = String::from_utf8_lossy(&std::fs::read(path)?).into_owned();
    let mut attested = HashSet::new();
    for line in text.lines() {
        let word = line.trim();
        if !word.is_empty() && !word.starts_with('#') {
            attested.insert(word.to_lowercase());
        }
    }
    Ok(attested)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cond(source: &str) -> Option<Cond> {
        Cond::parse(source)
    }

    #[test]
    fn conditions_match_suffix_and_prefix() {
        let c = cond("[^aeiou]").unwrap();
        assert!(c.matches_end("alm") && !c.matches_end("alma"));
        let c = cond("[^l]ak").unwrap();
        assert!(c.matches_end("barak") && !c.matches_end("balak"));
        assert!(cond("B-").unwrap().matches_end("B-"));
        assert!(cond("ny").unwrap().matches_start("nyak"));
        assert!(!cond("ny").unwrap().matches_start("nak"));
        assert!(cond("[áéiíoóőuúůüű-]").unwrap().matches_end("x-"));
    }

    #[test]
    fn reversed_class_ranges_are_dropped_like_python() {
        assert!(cond("[áéiíóőuúůüű-àùø]").is_none());
        assert!(cond("[áéiíoóőuúůüű-ø]").is_none());
        // érvényes tartomány
        assert!(cond("[áéiíoóőuúůüűy-ø]").is_some());
        assert!(cond("[a").is_none());
    }

    const AFF: &str = "SET UTF-8\nNEEDAFFIX u\nFORBIDDENWORD w\nONLYINCOMPOUND |\n\
        SFX A Y 2\nSFX A 0 ak .\nSFX A 0 ok [^a]\nSFX B Y 1\nSFX B 0 ban/A .\n\
        PFX P Y 1\nPFX P 0 meg .\n";
    const DIC: &str = "5\nalma/A\nkert/AB\nfalu/Pu\nmeg/P\ntilos/w\n";

    #[test]
    fn small_dictionary_roundtrip() {
        let checker = AffixChecker::from_bytes(AFF.as_bytes(), DIC.as_bytes(), None);
        assert!(checker.check("alma"));
        assert!(checker.check("almaak")); // alma + ak (a feltétel '.')
        assert!(!checker.check("almaok")); // az 'ok' végződés feltétele: [^a]
        assert!(checker.check("kertok"));
        assert!(checker.check("kert"));
        assert!(checker.check("kertak"));
        assert!(checker.check("kertban"));
        assert!(checker.check("kertbanak")); // két végződés: ban + ak
        assert!(!checker.check("tilos")); // tiltott szó
        assert!(!checker.check("xyz"));
        assert!(!checker.has_stem("falu")); // NEEDAFFIX
        assert_eq!(checker.entry_count(), 5);
    }
}
