mod builder;
mod graph;

pub use builder::{BuildError, GraphBuilder};
pub use graph::{ArcIter, GraphFormatError, NodeIdx, WordGraph};

use crate::rack::Rack;
use crate::rules::Alphabet;
use crate::tile::{Tile, MAX_LETTERS};
use alloc::string::String;
use alloc::vec::Vec;
use core::fmt;

pub const MAX_GADDAG_LETTERS: usize = MAX_LETTERS - 1;

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum LexiconError {
    UnknownCharacter { word: String, ch: char },
    EmptyWord,
    AlphabetTooLarge { len: usize, max: usize },
    Format(GraphFormatError),
    Build(BuildError),
}

impl fmt::Display for LexiconError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LexiconError::UnknownCharacter { word, ch } => {
                write!(
                    f,
                    "word {word:?} contains {ch:?}, which is not in the alphabet"
                )
            }
            LexiconError::EmptyWord => write!(f, "a lexicon cannot contain the empty word"),
            LexiconError::AlphabetTooLarge { len, max } => write!(
                f,
                "alphabet has {len} letters but a GADDAG allows at most {max}"
            ),
            LexiconError::Format(e) => write!(f, "{e}"),
            LexiconError::Build(e) => write!(f, "{e}"),
        }
    }
}

impl From<GraphFormatError> for LexiconError {
    fn from(e: GraphFormatError) -> Self {
        LexiconError::Format(e)
    }
}

impl From<BuildError> for LexiconError {
    fn from(e: BuildError) -> Self {
        LexiconError::Build(e)
    }
}

#[cfg(feature = "std")]
impl std::error::Error for LexiconError {}

#[derive(Clone, Debug)]
pub struct Lexicon {
    name: String,
    dawg: WordGraph,
    gaddag: WordGraph,
    separator: u8,
    alphabet_len: usize,
}

impl Lexicon {
    pub fn from_words<S: AsRef<str>>(
        alphabet: &Alphabet,
        words: impl IntoIterator<Item = S>,
    ) -> Result<Lexicon, LexiconError> {
        let mut encoded = Vec::new();
        for w in words {
            let w = w.as_ref().trim();
            if w.is_empty() {
                continue;
            }
            encoded.push(encode_word(alphabet, w)?);
        }
        Lexicon::from_encoded("lexicon", alphabet, encoded)
    }

    pub fn from_encoded(
        name: impl Into<String>,
        alphabet: &Alphabet,
        mut words: Vec<Vec<u8>>,
    ) -> Result<Lexicon, LexiconError> {
        let alphabet_len = alphabet.len();
        if alphabet_len > MAX_GADDAG_LETTERS {
            return Err(LexiconError::AlphabetTooLarge {
                len: alphabet_len,
                max: MAX_GADDAG_LETTERS,
            });
        }
        if words.iter().any(|w| w.is_empty()) {
            return Err(LexiconError::EmptyWord);
        }
        let separator = alphabet_len as u8;

        words.sort_unstable();
        words.dedup();

        let mut dawg = GraphBuilder::with_capacity(words.len());
        for w in &words {
            dawg.insert(w)?;
        }
        let dawg = dawg.finish();

        let gaddag = build_gaddag(&words, separator)?;

        Ok(Lexicon {
            name: name.into(),
            dawg,
            gaddag,
            separator,
            alphabet_len,
        })
    }

    pub fn from_graphs(
        name: impl Into<String>,
        dawg: WordGraph,
        gaddag: WordGraph,
        alphabet_len: usize,
    ) -> Lexicon {
        Lexicon {
            name: name.into(),
            dawg,
            gaddag,
            separator: alphabet_len as u8,
            alphabet_len,
        }
    }

    #[inline]
    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn with_name(mut self, name: impl Into<String>) -> Lexicon {
        self.name = name.into();
        self
    }

    #[inline]
    pub fn dawg(&self) -> &WordGraph {
        &self.dawg
    }

    #[inline]
    pub fn gaddag(&self) -> &WordGraph {
        &self.gaddag
    }

    #[inline]
    pub fn separator(&self) -> u8 {
        self.separator
    }

    #[inline]
    pub fn alphabet_len(&self) -> usize {
        self.alphabet_len
    }

    #[inline]
    pub fn letter_mask(&self) -> u64 {
        if self.alphabet_len >= 64 {
            u64::MAX
        } else {
            (1u64 << self.alphabet_len) - 1
        }
    }

    #[inline]
    pub fn word_count(&self) -> u64 {
        self.dawg.word_count()
    }

    #[inline]
    pub fn size_bytes(&self) -> usize {
        self.dawg.size_bytes() + self.gaddag.size_bytes()
    }

    #[inline]
    pub fn contains(&self, word: &[u8]) -> bool {
        !word.is_empty() && self.dawg.accepts_sequence(word.iter().copied())
    }

    pub fn contains_str(&self, alphabet: &Alphabet, word: &str) -> bool {
        match encode_word(alphabet, word) {
            Ok(seq) => self.contains(&seq),
            Err(_) => false,
        }
    }

    pub fn cross_set(&self, prefix: &[u8], suffix: &[u8]) -> u64 {
        if prefix.is_empty() && suffix.is_empty() {
            return self.letter_mask();
        }
        let Some(node) = self.dawg.walk(self.dawg.root(), prefix.iter().copied()) else {
            return 0;
        };
        let mut set = 0u64;
        for letter in self.dawg.arcs(node) {
            let next = self.dawg.child_unchecked(node, letter);
            if let Some(end) = self.dawg.walk(next, suffix.iter().copied()) {
                if self.dawg.accepts(end) {
                    set |= 1u64 << letter;
                }
            }
        }
        set
    }

    pub fn anagrams(&self, rack: &Rack) -> Vec<Vec<u8>> {
        let mut out = Vec::new();
        let mut stack = Vec::with_capacity(rack.len());
        let mut counts = *rack.counts();
        let blanks = rack.blanks();
        self.anagram_walk(self.dawg.root(), &mut counts, blanks, &mut stack, &mut out);
        out
    }

    fn anagram_walk(
        &self,
        node: NodeIdx,
        counts: &mut [u8; crate::tile::TILE_CODES],
        blanks: u8,
        stack: &mut Vec<u8>,
        out: &mut Vec<Vec<u8>>,
    ) {
        if self.dawg.accepts(node) {
            out.push(stack.clone());
        }
        let mut children = self.dawg.children(node);
        while children != 0 {
            let letter = children.trailing_zeros() as u8;
            children &= children - 1;
            let next = self.dawg.child_unchecked(node, letter);

            if counts[letter as usize] > 0 {
                counts[letter as usize] -= 1;
                stack.push(letter);
                self.anagram_walk(next, counts, blanks, stack, out);
                stack.pop();
                counts[letter as usize] += 1;
            } else if blanks > 0 {
                stack.push(letter);
                self.anagram_walk(next, counts, blanks - 1, stack, out);
                stack.pop();
            }
        }
    }

    pub fn decode(&self, alphabet: &Alphabet, word: &[u8]) -> String {
        word.iter().map(|&l| alphabet.display(l)).collect()
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let dawg = self.dawg.to_bytes();
        let gaddag = self.gaddag.to_bytes();
        let name = self.name.as_bytes();

        let mut out = Vec::with_capacity(dawg.len() + gaddag.len() + name.len() + 32);
        out.extend_from_slice(b"SLEX");
        out.push(self.alphabet_len as u8);
        out.push(self.separator);
        out.extend_from_slice(&(name.len() as u16).to_le_bytes());
        out.extend_from_slice(name);
        out.extend_from_slice(&(dawg.len() as u64).to_le_bytes());
        out.extend_from_slice(&(gaddag.len() as u64).to_le_bytes());
        out.extend_from_slice(&dawg);
        out.extend_from_slice(&gaddag);
        out
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Lexicon, LexiconError> {
        let need = |have: usize, want: usize| -> Result<(), LexiconError> {
            if have < want {
                Err(LexiconError::Format(GraphFormatError::Truncated))
            } else {
                Ok(())
            }
        };
        need(bytes.len(), 8)?;
        if &bytes[..4] != b"SLEX" {
            return Err(LexiconError::Format(GraphFormatError::BadMagic));
        }
        let alphabet_len = bytes[4] as usize;
        let separator = bytes[5];
        let name_len = u16::from_le_bytes([bytes[6], bytes[7]]) as usize;

        let mut at = 8;
        need(bytes.len(), at + name_len + 16)?;
        let name = String::from_utf8_lossy(&bytes[at..at + name_len]).into_owned();
        at += name_len;

        let read_u64 = |at: usize| {
            let mut b = [0u8; 8];
            b.copy_from_slice(&bytes[at..at + 8]);
            u64::from_le_bytes(b) as usize
        };
        let dawg_len = read_u64(at);
        let gaddag_len = read_u64(at + 8);
        at += 16;

        need(bytes.len(), at + dawg_len + gaddag_len)?;
        let dawg = WordGraph::from_bytes(&bytes[at..at + dawg_len])?;
        let gaddag = WordGraph::from_bytes(&bytes[at + dawg_len..at + dawg_len + gaddag_len])?;

        Ok(Lexicon {
            name,
            dawg,
            gaddag,
            separator,
            alphabet_len,
        })
    }

    pub fn validate(&self) -> Result<(), LexiconError> {
        self.dawg.validate()?;
        self.gaddag.validate()?;
        Ok(())
    }
}

pub fn encode_word(alphabet: &Alphabet, word: &str) -> Result<Vec<u8>, LexiconError> {
    if word.is_empty() {
        return Err(LexiconError::EmptyWord);
    }
    let mut out = Vec::with_capacity(word.len());
    for ch in word.chars() {
        let (letter, _) =
            alphabet
                .parse_char(ch)
                .ok_or_else(|| LexiconError::UnknownCharacter {
                    word: word.into(),
                    ch,
                })?;
        out.push(letter);
    }
    Ok(out)
}

fn build_gaddag(words: &[Vec<u8>], separator: u8) -> Result<WordGraph, LexiconError> {
    let total: usize = words.iter().map(|w| w.len() * (w.len() + 1)).sum();
    let count: usize = words.iter().map(|w| w.len()).sum();

    let mut flat: Vec<u8> = Vec::with_capacity(total);
    let mut spans: Vec<(u32, u32)> = Vec::with_capacity(count);

    for word in words {
        for split in 1..=word.len() {
            let start = flat.len() as u32;
            flat.extend(word[..split].iter().rev());
            flat.push(separator);
            flat.extend_from_slice(&word[split..]);
            spans.push((start, flat.len() as u32 - start));
        }
    }

    let at = |&(s, l): &(u32, u32)| -> &[u8] { &flat[s as usize..(s + l) as usize] };
    spans.sort_unstable_by(|a, b| at(a).cmp(at(b)));

    let mut builder = GraphBuilder::with_capacity(spans.len());
    for span in &spans {
        builder.insert(at(span))?;
    }
    Ok(builder.finish())
}

#[inline]
pub fn tile_for(letter: u8) -> Tile {
    Tile::letter(letter)
}

#[cfg(test)]
mod tests {
    use super::*;

    const WORDS: [&str; 10] = [
        "AT", "ATE", "CAT", "CATS", "EAT", "EATS", "SAT", "SATE", "TA", "TAE",
    ];

    fn lex() -> Lexicon {
        Lexicon::from_words(&Alphabet::english(), WORDS).expect("test words are all valid")
    }

    fn seq(w: &str) -> Vec<u8> {
        w.bytes().map(|c| c - b'A').collect()
    }

    #[test]
    fn contains_exactly_the_input_words() {
        let l = lex();
        assert_eq!(l.word_count(), WORDS.len() as u64);
        for w in WORDS {
            assert!(l.contains(&seq(w)), "{w} should be a word");
        }
        for w in ["A", "C", "CA", "CATT", "TEA", "ETA"] {
            assert!(!l.contains(&seq(w)), "{w} should not be a word");
        }
        assert!(!l.contains(&[]));
    }

    #[test]
    fn contains_str_is_case_insensitive_and_safe() {
        let l = lex();
        let a = Alphabet::english();
        assert!(l.contains_str(&a, "cat"));
        assert!(l.contains_str(&a, "CAT"));
        assert!(!l.contains_str(&a, "c4t"), "bad characters are not words");
        assert!(!l.contains_str(&a, ""));
    }

    #[test]
    fn duplicates_and_case_collapse() {
        let l = Lexicon::from_words(&Alphabet::english(), ["cat", "CAT", "Cat"]).unwrap();
        assert_eq!(l.word_count(), 1);
    }

    #[test]
    fn the_gaddag_holds_every_split_of_every_word() {
        let l = lex();
        let sep = l.separator();
        let g = l.gaddag();
        for w in WORDS {
            let word = seq(w);
            for split in 1..=word.len() {
                let mut path: Vec<u8> = word[..split].iter().rev().copied().collect();
                path.push(sep);
                path.extend_from_slice(&word[split..]);
                assert!(
                    g.accepts_sequence(path.iter().copied()),
                    "GADDAG is missing the split of {w} at {split}"
                );
            }
        }
    }

    #[test]
    fn the_gaddag_rejects_paths_for_non_words() {
        let l = lex();
        let sep = l.separator();

        let word = seq("TEA");
        for split in 1..=word.len() {
            let mut path: Vec<u8> = word[..split].iter().rev().copied().collect();
            path.push(sep);
            path.extend_from_slice(&word[split..]);
            assert!(!l.gaddag().accepts_sequence(path));
        }
    }

    #[test]
    fn the_gaddag_is_much_bigger_than_the_dawg() {
        let l = lex();
        assert!(
            l.gaddag().node_count() > l.dawg().node_count(),
            "a GADDAG stores every split, so it cannot be smaller"
        );
    }

    #[test]
    fn cross_set_finds_the_letters_that_complete_a_word() {
        let l = lex();

        let set = l.cross_set(&[], &seq("AT"));
        let mut got: Vec<u8> = (0..26u8).filter(|&b| set & (1u64 << b) != 0).collect();
        got.sort_unstable();
        assert_eq!(got, seq("CES"), "expected C, E and S");

        let set = l.cross_set(&seq("CA"), &[]);
        assert_eq!(set, 1u64 << seq("T")[0]);

        let set = l.cross_set(&seq("A"), &seq("E"));
        assert_eq!(set, 1u64 << seq("T")[0]);
    }

    #[test]
    fn cross_set_is_empty_when_nothing_fits() {
        let l = lex();
        assert_eq!(l.cross_set(&seq("Z"), &seq("Z")), 0);
        assert_eq!(l.cross_set(&seq("QQQ"), &[]), 0);
    }

    #[test]
    fn cross_set_of_an_isolated_square_is_every_letter() {
        let l = lex();
        assert_eq!(l.cross_set(&[], &[]), l.letter_mask());
        assert_eq!(l.letter_mask().count_ones(), 26);
    }

    #[test]
    fn cross_set_agrees_with_brute_force() {
        let l = lex();
        let cases: [(&str, &str); 6] = [
            ("", "AT"),
            ("CA", ""),
            ("A", "E"),
            ("S", "T"),
            ("", "A"),
            ("EA", "S"),
        ];
        for (p, s) in cases {
            let (p, s) = (seq(p), seq(s));
            let mut want = 0u64;
            for letter in 0..26u8 {
                let mut word = p.clone();
                word.push(letter);
                word.extend_from_slice(&s);
                if l.contains(&word) {
                    want |= 1u64 << letter;
                }
            }
            assert_eq!(l.cross_set(&p, &s), want, "mismatch for {p:?}_{s:?}");
        }
    }

    #[test]
    fn anagrams_finds_every_makeable_word() {
        let l = lex();
        let a = Alphabet::english();
        let rack = Rack::parse(&a, "CATS").unwrap();
        let mut got: Vec<String> = l.anagrams(&rack).iter().map(|w| l.decode(&a, w)).collect();
        got.sort();
        assert_eq!(got, ["AT", "CAT", "CATS", "SAT", "TA"]);
    }

    #[test]
    fn anagrams_uses_blanks_without_duplicating_words() {
        let l = lex();
        let a = Alphabet::english();
        let rack = Rack::parse(&a, "AT?").unwrap();
        let got: Vec<String> = l.anagrams(&rack).iter().map(|w| l.decode(&a, w)).collect();
        let mut sorted = got.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(got.len(), sorted.len(), "no word may be emitted twice");
        assert!(
            sorted.contains(&String::from("ATE")),
            "the blank should reach ATE"
        );
        assert!(
            sorted.contains(&String::from("CAT")),
            "the blank should reach CAT"
        );
    }

    #[test]
    fn anagrams_of_an_empty_rack_are_empty() {
        let l = lex();
        assert!(l.anagrams(&Rack::new()).is_empty());
    }

    #[test]
    fn bytes_round_trip() {
        let l = lex().with_name("test-lexicon");
        let bytes = l.to_bytes();
        let back = Lexicon::from_bytes(&bytes).expect("round trip should parse");
        assert_eq!(back.name(), "test-lexicon");
        assert_eq!(back.word_count(), l.word_count());
        assert_eq!(back.separator(), l.separator());
        assert_eq!(back.alphabet_len(), l.alphabet_len());
        for w in WORDS {
            assert!(back.contains(&seq(w)));
        }
        back.validate().expect("a round-tripped lexicon is valid");
    }

    #[test]
    fn from_bytes_rejects_junk() {
        assert!(matches!(
            Lexicon::from_bytes(b"nope"),
            Err(LexiconError::Format(GraphFormatError::Truncated))
        ));
        assert!(matches!(
            Lexicon::from_bytes(b"XXXX\0\0\0\0"),
            Err(LexiconError::Format(GraphFormatError::BadMagic))
        ));
    }

    #[test]
    fn unknown_characters_are_reported_with_context() {
        let err = Lexicon::from_words(&Alphabet::english(), ["CAT", "DO6"]).unwrap_err();
        assert_eq!(
            err,
            LexiconError::UnknownCharacter {
                word: "DO6".into(),
                ch: '6'
            }
        );
    }

    #[test]
    fn blank_lines_are_skipped_not_rejected() {
        let l = Lexicon::from_words(&Alphabet::english(), ["CAT", "", "  ", "DOG"]).unwrap();
        assert_eq!(l.word_count(), 2);
    }

    #[test]
    fn a_single_letter_word_survives_gaddag_construction() {
        let l = Lexicon::from_words(&Alphabet::english(), ["A", "AT"]).unwrap();
        assert!(l.contains(&seq("A")));
        let sep = l.separator();
        assert!(l.gaddag().accepts_sequence(alloc::vec![seq("A")[0], sep]));
    }

    #[test]
    fn graphs_validate_after_construction() {
        lex()
            .validate()
            .expect("freshly built graphs must be valid");
    }
}
