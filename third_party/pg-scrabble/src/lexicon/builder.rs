use super::graph::{NodeIdx, WordGraph};
use crate::tile::MAX_LETTERS;
use alloc::vec::Vec;
use core::fmt;

#[cfg(feature = "std")]
type Register = std::collections::HashMap<u64, Vec<NodeIdx>>;
#[cfg(not(feature = "std"))]
type Register = alloc::collections::BTreeMap<u64, Vec<NodeIdx>>;

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum BuildError {
    NotSorted { got: Vec<u8>, after: Vec<u8> },
    LetterOutOfRange(u8),
    EmptySequence,
}

impl fmt::Display for BuildError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BuildError::NotSorted { got, after } => {
                write!(f, "input must be sorted: {got:?} arrived after {after:?}")
            }
            BuildError::LetterOutOfRange(l) => {
                write!(
                    f,
                    "letter index {l} is at or above the maximum of {MAX_LETTERS}"
                )
            }
            BuildError::EmptySequence => write!(f, "the empty sequence cannot be a word"),
        }
    }
}

#[cfg(feature = "std")]
impl std::error::Error for BuildError {}

#[derive(Clone, Debug)]
struct BuildNode {
    accepts: bool,
    children: Vec<(u8, NodeIdx)>,
}

impl BuildNode {
    fn new() -> BuildNode {
        BuildNode {
            accepts: false,
            children: Vec::new(),
        }
    }
}

pub struct GraphBuilder {
    data: Vec<u32>,
    register: Register,
    path: Vec<BuildNode>,
    letters: Vec<u8>,
    prev: Vec<u8>,
    word_count: u64,
    node_count: u32,
    arc_count: u32,
}

impl GraphBuilder {
    pub fn new() -> GraphBuilder {
        GraphBuilder {
            data: Vec::new(),
            register: Register::new(),
            path: alloc::vec![BuildNode::new()],
            letters: Vec::new(),
            prev: Vec::new(),
            word_count: 0,
            node_count: 0,
            arc_count: 0,
        }
    }

    pub fn with_capacity(words: usize) -> GraphBuilder {
        let mut b = GraphBuilder::new();

        b.data.reserve(words * 5);
        #[cfg(feature = "std")]
        b.register.reserve(words);
        b
    }

    #[inline]
    pub fn word_count(&self) -> u64 {
        self.word_count
    }

    pub fn insert(&mut self, seq: &[u8]) -> Result<(), BuildError> {
        if seq.is_empty() {
            return Err(BuildError::EmptySequence);
        }
        if let Some(&bad) = seq.iter().find(|&&l| l as usize >= MAX_LETTERS) {
            return Err(BuildError::LetterOutOfRange(bad));
        }
        match seq.cmp(&self.prev[..]) {
            core::cmp::Ordering::Less => {
                return Err(BuildError::NotSorted {
                    got: seq.to_vec(),
                    after: self.prev.clone(),
                })
            }
            core::cmp::Ordering::Equal if !self.prev.is_empty() => return Ok(()),
            _ => {}
        }

        let prefix = common_prefix(seq, &self.prev);
        self.collapse_to(prefix);

        for &letter in &seq[prefix..] {
            self.letters.push(letter);
            self.path.push(BuildNode::new());
        }
        self.path
            .last_mut()
            .expect("the path always holds at least the root")
            .accepts = true;

        self.prev.clear();
        self.prev.extend_from_slice(seq);
        self.word_count += 1;
        Ok(())
    }

    pub fn extend_sorted<'a>(
        &mut self,
        seqs: impl IntoIterator<Item = &'a [u8]>,
    ) -> Result<(), BuildError> {
        for s in seqs {
            self.insert(s)?;
        }
        Ok(())
    }

    pub fn finish(mut self) -> WordGraph {
        self.collapse_to(0);
        let root_node = self.path.pop().expect("the root is always present");
        let root = self.commit(root_node);
        WordGraph::from_parts(
            self.data,
            root,
            self.node_count,
            self.arc_count,
            self.word_count,
        )
        .reordered()
    }

    fn collapse_to(&mut self, depth: usize) {
        while self.path.len() > depth + 1 {
            let node = self.path.pop().expect("checked by the loop condition");
            let letter = self.letters.pop().expect("one letter per non-root node");
            let id = self.commit(node);
            self.path
                .last_mut()
                .expect("popping never empties the path")
                .children
                .push((letter, id));
        }
    }

    fn commit(&mut self, mut node: BuildNode) -> NodeIdx {
        node.children.sort_unstable_by_key(|&(l, _)| l);
        debug_assert!(
            node.children.windows(2).all(|w| w[0].0 < w[1].0),
            "a node cannot have two arcs for the same letter"
        );

        let mut mask = 0u64;
        for &(l, _) in &node.children {
            mask |= 1u64 << l;
        }
        let header = if node.accepts {
            mask | (1u64 << 63)
        } else {
            mask
        };

        let hash = structural_hash(header, &node.children);
        if let Some(candidates) = self.register.get(&hash) {
            for &offset in candidates {
                if self.matches(offset, header, &node.children) {
                    return offset;
                }
            }
        }

        let offset = self.data.len() as NodeIdx;
        self.data.push(header as u32);
        self.data.push((header >> 32) as u32);
        for &(_, target) in &node.children {
            self.data.push(target);
        }
        self.node_count += 1;
        self.arc_count += node.children.len() as u32;
        self.register.entry(hash).or_default().push(offset);
        offset
    }

    fn matches(&self, offset: NodeIdx, header: u64, children: &[(u8, NodeIdx)]) -> bool {
        let base = offset as usize;
        if self.data[base] != header as u32 || self.data[base + 1] != (header >> 32) as u32 {
            return false;
        }

        self.data[base + 2..base + 2 + children.len()]
            .iter()
            .zip(children.iter())
            .all(|(&got, &(_, want))| got == want)
    }
}

impl Default for GraphBuilder {
    fn default() -> Self {
        GraphBuilder::new()
    }
}

fn common_prefix(a: &[u8], b: &[u8]) -> usize {
    a.iter().zip(b.iter()).take_while(|(x, y)| x == y).count()
}

fn structural_hash(header: u64, children: &[(u8, NodeIdx)]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut mix = |v: u64| {
        h ^= v;
        h = h.wrapping_mul(0x0100_0000_01b3);
    };
    mix(header);
    for &(_, target) in children {
        mix(target as u64);
    }
    h
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seq(w: &str) -> Vec<u8> {
        w.bytes().map(|c| c - b'A').collect()
    }

    fn build(words: &[&str]) -> WordGraph {
        let mut sorted: Vec<Vec<u8>> = words.iter().map(|w| seq(w)).collect();
        sorted.sort();
        let mut b = GraphBuilder::new();
        for s in &sorted {
            b.insert(s).unwrap();
        }
        b.finish()
    }

    #[test]
    fn accepts_exactly_the_input() {
        let words = ["AB", "ABC", "ABCD", "BC", "CD", "CDE"];
        let g = build(&words);
        let got: Vec<Vec<u8>> = g.collect_words();
        let mut want: Vec<Vec<u8>> = words.iter().map(|w| seq(w)).collect();
        want.sort();
        assert_eq!(got, want);
        assert_eq!(g.word_count(), words.len() as u64);
    }

    #[test]
    fn a_single_word_graph_is_a_chain() {
        let g = build(&["CAT"]);
        assert!(g.accepts_sequence(seq("CAT")));
        assert!(!g.accepts_sequence(seq("CA")));

        assert_eq!(g.node_count(), 4);
        assert_eq!(g.arc_count(), 3);
    }

    #[test]
    fn duplicates_are_ignored() {
        let mut b = GraphBuilder::new();
        b.insert(&seq("CAT")).unwrap();
        b.insert(&seq("CAT")).unwrap();
        b.insert(&seq("CATS")).unwrap();
        let g = b.finish();
        assert_eq!(g.word_count(), 2);
        assert_eq!(g.collect_words().len(), 2);
    }

    #[test]
    fn unsorted_input_is_rejected() {
        let mut b = GraphBuilder::new();
        b.insert(&seq("CAT")).unwrap();
        let err = b.insert(&seq("AT")).unwrap_err();
        assert!(matches!(err, BuildError::NotSorted { .. }));
    }

    #[test]
    fn bad_letters_and_empty_sequences_are_rejected() {
        let mut b = GraphBuilder::new();
        assert_eq!(b.insert(&[]), Err(BuildError::EmptySequence));
        assert_eq!(b.insert(&[0, 99]), Err(BuildError::LetterOutOfRange(99)));
        assert_eq!(b.word_count(), 0);
    }

    #[test]
    fn an_empty_builder_finishes_into_an_empty_graph() {
        let g = GraphBuilder::new().finish();
        assert_eq!(g.word_count(), 0);
        assert!(g.collect_words().is_empty());
        assert!(!g.accepts_sequence(seq("A")));
    }

    #[test]
    fn the_result_is_actually_minimal() {
        let g = build(&["DOING", "GOING", "SUING", "TAKING"]);
        assert!(
            g.node_count() <= 16,
            "expected suffix sharing, got {} nodes",
            g.node_count()
        );

        let a = build(&["AB", "AC", "BB", "BC"]);
        let b = build(&["BC", "BB", "AC", "AB"]);
        assert_eq!(a.node_count(), b.node_count());
        assert_eq!(a.collect_words(), b.collect_words());
    }

    #[test]
    fn equivalent_subtrees_are_shared_not_duplicated() {
        let g = build(&["XY", "ZY"]);
        let root = g.root();
        let x = g.child(root, seq("X")[0]).unwrap();
        let z = g.child(root, seq("Z")[0]).unwrap();
        assert_eq!(x, z, "identical subtrees must be the same node");
    }

    #[test]
    fn prefix_words_are_marked_accepting() {
        let g = build(&["A", "AB", "ABC"]);
        assert!(g.accepts_sequence(seq("A")));
        assert!(g.accepts_sequence(seq("AB")));
        assert!(g.accepts_sequence(seq("ABC")));
        assert_eq!(g.collect_words().len(), 3);
    }

    #[test]
    fn handles_the_full_letter_range() {
        let wide: Vec<u8> = (0..MAX_LETTERS as u8).collect();
        let mut b = GraphBuilder::new();
        b.insert(&wide).unwrap();
        let single: Vec<u8> = alloc::vec![MAX_LETTERS as u8 - 1];
        b.insert(&single).unwrap();
        let g = b.finish();
        assert!(g.accepts_sequence(wide));
        assert!(g.accepts_sequence(single));
        assert_eq!(g.degree(g.root()), 2);
    }

    #[test]
    fn a_node_can_hold_arcs_on_both_sides_of_the_old_32_bit_boundary() {
        let top = MAX_LETTERS as u8 - 1;
        let partner = |l: u8| ((l as usize * 7 + 3) % MAX_LETTERS) as u8;
        let mut words: Vec<Vec<u8>> = (0..MAX_LETTERS as u8)
            .map(|l| alloc::vec![l, partner(l)])
            .collect();
        words.sort();
        let mut b = GraphBuilder::new();
        for w in &words {
            b.insert(w).unwrap();
        }
        let g = b.finish();
        g.validate().unwrap();
        assert_eq!(g.degree(g.root()) as usize, MAX_LETTERS);
        assert_eq!(g.collect_words(), words);
        assert!(g.accepts_sequence([top, partner(top)]));
        assert_eq!(
            b_err(&[top + 1]),
            BuildError::LetterOutOfRange(top + 1),
            "the first index past the limit is rejected"
        );
    }

    fn b_err(seq: &[u8]) -> BuildError {
        GraphBuilder::new().insert(seq).unwrap_err()
    }

    #[test]
    fn a_larger_graph_validates_and_round_trips() {
        let mut words: Vec<Vec<u8>> = Vec::new();
        for a in 0..12u8 {
            for b in 0..12u8 {
                for c in 0..8u8 {
                    words.push(alloc::vec![a, b, c]);
                }
            }
        }
        words.sort();
        let mut builder = GraphBuilder::with_capacity(words.len());
        for w in &words {
            builder.insert(w).unwrap();
        }
        let g = builder.finish();
        assert_eq!(g.word_count(), words.len() as u64);
        g.validate()
            .expect("builder output must be structurally valid");
        assert_eq!(g.collect_words(), words);
    }
}
