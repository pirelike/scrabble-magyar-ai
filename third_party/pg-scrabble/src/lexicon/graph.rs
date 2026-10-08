use crate::tile::MAX_LETTERS;
use alloc::vec::Vec;
use core::fmt;

pub type NodeIdx = u32;

/// Layout of the flat `u32` word array.
///
/// Every node is a two-word header followed by one word per outgoing arc:
///
/// ```text
/// data[n]     = low  32 bits of the header
/// data[n + 1] = high 32 bits of the header
/// data[n + 2 + k] = target node of the k-th arc (arcs sorted by letter)
/// ```
///
/// The 64-bit header is `ACCEPT_BIT | children`, where bit `l` of `children`
/// is set when the node has an arc for letter `l` (`l` in `0..MAX_LETTERS`).
/// Upstream `scrabble` 0.1.0 used a single 32-bit header word (31 letters).
const HEADER_WORDS: usize = 2;

const ACCEPT_BIT: u64 = 1 << 63;
const CHILD_MASK: u64 = !ACCEPT_BIT;

const _: () = assert!(MAX_LETTERS <= 63, "children mask must leave bit 63 free");

#[derive(Clone, PartialEq, Eq)]
pub struct WordGraph {
    data: Vec<u32>,
    root: NodeIdx,
    node_count: u32,
    arc_count: u32,
    word_count: u64,
}

impl WordGraph {
    pub(crate) fn from_parts(
        data: Vec<u32>,
        root: NodeIdx,
        node_count: u32,
        arc_count: u32,
        word_count: u64,
    ) -> WordGraph {
        debug_assert!(
            data.len() >= HEADER_WORDS,
            "a graph always has at least a root header"
        );
        debug_assert!((root as usize) < data.len(), "root is out of range");
        WordGraph {
            data,
            root,
            node_count,
            arc_count,
            word_count,
        }
    }

    pub fn empty() -> WordGraph {
        WordGraph {
            data: alloc::vec![0; HEADER_WORDS],
            root: 0,
            node_count: 1,
            arc_count: 0,
            word_count: 0,
        }
    }

    #[inline]
    pub const fn root(&self) -> NodeIdx {
        self.root
    }

    #[inline]
    pub fn node_count(&self) -> u32 {
        self.node_count
    }

    #[inline]
    pub fn arc_count(&self) -> u32 {
        self.arc_count
    }

    #[inline]
    pub fn word_count(&self) -> u64 {
        self.word_count
    }

    #[inline]
    pub fn size_bytes(&self) -> usize {
        self.data.len() * core::mem::size_of::<u32>()
    }

    #[inline]
    fn header_at(&self, node: usize) -> u64 {
        // one bounds check, one 64-bit load
        let h = &self.data[node..node + HEADER_WORDS];
        (h[0] as u64) | ((h[1] as u64) << 32)
    }

    #[inline]
    pub fn children(&self, node: NodeIdx) -> u64 {
        self.header_at(node as usize) & CHILD_MASK
    }

    #[inline]
    pub fn accepts(&self, node: NodeIdx) -> bool {
        self.header_at(node as usize) & ACCEPT_BIT != 0
    }

    #[inline]
    pub fn header(&self, node: NodeIdx) -> u64 {
        self.header_at(node as usize)
    }

    #[inline]
    pub const fn split_header(header: u64) -> (bool, u64) {
        (header & ACCEPT_BIT != 0, header & CHILD_MASK)
    }

    #[inline]
    pub fn degree(&self, node: NodeIdx) -> u32 {
        self.children(node).count_ones()
    }

    #[inline]
    pub fn child(&self, node: NodeIdx, letter: u8) -> Option<NodeIdx> {
        debug_assert!((letter as usize) < MAX_LETTERS);
        let children = self.children(node);
        if children & (1u64 << letter) == 0 {
            None
        } else {
            Some(self.child_for(node, children, letter))
        }
    }

    #[inline]
    pub fn child_unchecked(&self, node: NodeIdx, letter: u8) -> NodeIdx {
        let children = self.children(node);
        debug_assert!(
            children & (1u64 << letter) != 0,
            "child_unchecked called for a letter with no arc"
        );
        self.child_for(node, children, letter)
    }

    #[inline]
    pub fn child_with_mask(&self, node: NodeIdx, children: u64, letter: u8) -> NodeIdx {
        debug_assert_eq!(children, self.children(node), "stale child mask");
        self.child_for(node, children, letter)
    }

    #[inline]
    fn child_for(&self, node: NodeIdx, children: u64, letter: u8) -> NodeIdx {
        let rank = (children & ((1u64 << letter) - 1)).count_ones();
        self.data[node as usize + HEADER_WORDS + rank as usize]
    }

    pub fn reordered(&self) -> WordGraph {
        let len = self.data.len();
        let mut new_offset = alloc::vec![u32::MAX; len];
        let mut visited = alloc::vec![false; len];
        let mut order: Vec<NodeIdx> = Vec::with_capacity(self.node_count as usize);
        let mut queue = alloc::collections::VecDeque::new();

        visited[self.root as usize] = true;
        queue.push_back(self.root);
        let mut cursor: u32 = 0;

        while let Some(node) = queue.pop_front() {
            new_offset[node as usize] = cursor;
            let degree = (self.header_at(node as usize) & CHILD_MASK).count_ones() as usize;
            cursor += (HEADER_WORDS + degree) as u32;
            order.push(node);
            for i in 0..degree {
                let target = self.data[node as usize + HEADER_WORDS + i];
                if !visited[target as usize] {
                    visited[target as usize] = true;
                    queue.push_back(target);
                }
            }
        }

        let mut data = alloc::vec![0u32; cursor as usize];
        let mut arcs = 0u32;
        for &node in &order {
            let from = node as usize;
            let to = new_offset[from] as usize;
            data[to] = self.data[from];
            data[to + 1] = self.data[from + 1];
            let degree = (self.header_at(from) & CHILD_MASK).count_ones() as usize;
            arcs += degree as u32;
            for i in 0..degree {
                data[to + HEADER_WORDS + i] =
                    new_offset[self.data[from + HEADER_WORDS + i] as usize];
            }
        }

        WordGraph {
            data,
            root: 0,
            node_count: order.len() as u32,
            arc_count: arcs,
            word_count: self.word_count,
        }
    }

    pub fn walk(&self, node: NodeIdx, letters: impl IntoIterator<Item = u8>) -> Option<NodeIdx> {
        let mut cur = node;
        for l in letters {
            cur = self.child(cur, l)?;
        }
        Some(cur)
    }

    pub fn accepts_sequence(&self, letters: impl IntoIterator<Item = u8>) -> bool {
        match self.walk(self.root, letters) {
            Some(n) => self.accepts(n),
            None => false,
        }
    }

    #[inline]
    pub fn arcs(&self, node: NodeIdx) -> ArcIter {
        ArcIter {
            remaining: self.children(node),
        }
    }

    pub fn collect_words(&self) -> Vec<Vec<u8>> {
        let mut out = Vec::new();
        let mut stack = Vec::new();
        self.collect_from(self.root, &mut stack, &mut out);
        out
    }

    fn collect_from(&self, node: NodeIdx, stack: &mut Vec<u8>, out: &mut Vec<Vec<u8>>) {
        if self.accepts(node) {
            out.push(stack.clone());
        }
        for letter in self.arcs(node) {
            stack.push(letter);
            self.collect_from(self.child_unchecked(node, letter), stack, out);
            stack.pop();
        }
    }

    /// The raw word array (see the layout description at the top of this file).
    #[inline]
    pub fn raw(&self) -> &[u32] {
        &self.data
    }

    /// Magic of the current serialised format: 64-bit headers (two words per
    /// node header). Upstream `scrabble` 0.1.0 wrote [`Self::MAGIC_V1`].
    pub const MAGIC: [u8; 4] = *b"SWG2";

    /// Magic of the upstream `scrabble` 0.1.0 format (32-bit headers, at most
    /// 31 letters). Still accepted by [`WordGraph::from_bytes`], which widens
    /// it on load; it is never written.
    pub const MAGIC_V1: [u8; 4] = *b"SWG1";

    const HEADER_LEN: usize = 28;

    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(Self::HEADER_LEN + self.data.len() * 4);
        out.extend_from_slice(&Self::MAGIC);
        out.extend_from_slice(&self.node_count.to_le_bytes());
        out.extend_from_slice(&self.arc_count.to_le_bytes());
        out.extend_from_slice(&(self.data.len() as u32).to_le_bytes());
        out.extend_from_slice(&self.root.to_le_bytes());
        out.extend_from_slice(&self.word_count.to_le_bytes());
        for w in &self.data {
            out.extend_from_slice(&w.to_le_bytes());
        }
        out
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<WordGraph, GraphFormatError> {
        if bytes.len() < Self::HEADER_LEN {
            return Err(GraphFormatError::Truncated);
        }
        let legacy = if bytes[..4] == Self::MAGIC {
            false
        } else if bytes[..4] == Self::MAGIC_V1 {
            true
        } else {
            return Err(GraphFormatError::BadMagic);
        };
        let u32_at =
            |i: usize| u32::from_le_bytes([bytes[i], bytes[i + 1], bytes[i + 2], bytes[i + 3]]);
        let node_count = u32_at(4);
        let arc_count = u32_at(8);
        let len = u32_at(12) as usize;
        let root = u32_at(16);
        let mut wc = [0u8; 8];
        wc.copy_from_slice(&bytes[20..28]);
        let word_count = u64::from_le_bytes(wc);

        if len == 0 || root as usize >= len {
            return Err(GraphFormatError::Corrupt);
        }
        if bytes.len() < Self::HEADER_LEN + len * 4 {
            return Err(GraphFormatError::Truncated);
        }
        let mut data = Vec::with_capacity(len);
        for i in 0..len {
            data.push(u32_at(Self::HEADER_LEN + i * 4));
        }
        if legacy {
            return Self::widen_v1(data, root, word_count);
        }
        if len < HEADER_WORDS {
            return Err(GraphFormatError::Corrupt);
        }
        Ok(WordGraph {
            data,
            root,
            node_count,
            arc_count,
            word_count,
        })
    }

    /// Convert a version-1 word array (one 32-bit header word per node, bit 31
    /// = accepts, bits 0..=30 = children) into the current layout. The nodes are
    /// laid out breadth-first from the root, which is exactly what
    /// [`WordGraph::reordered`] (and therefore the builder) produces.
    fn widen_v1(old: Vec<u32>, root: u32, word_count: u64) -> Result<WordGraph, GraphFormatError> {
        const OLD_ACCEPT: u32 = 1 << 31;
        const OLD_CHILDREN: u32 = !OLD_ACCEPT;
        let len = old.len();

        let mut new_offset = alloc::vec![u32::MAX; len];
        let mut order: Vec<usize> = Vec::new();
        let mut queue = alloc::collections::VecDeque::new();
        new_offset[root as usize] = 0;
        queue.push_back(root as usize);
        let mut cursor = 0usize;
        let mut arcs = 0u32;
        while let Some(n) = queue.pop_front() {
            let degree = (old[n] & OLD_CHILDREN).count_ones() as usize;
            if n + degree >= len {
                return Err(GraphFormatError::Corrupt);
            }
            new_offset[n] = cursor as u32;
            cursor += HEADER_WORDS + degree;
            arcs += degree as u32;
            order.push(n);
            for i in 0..degree {
                let target = old[n + 1 + i] as usize;
                if target >= len {
                    return Err(GraphFormatError::Corrupt);
                }
                if new_offset[target] == u32::MAX {
                    new_offset[target] = u32::MAX - 1;
                    queue.push_back(target);
                }
            }
        }

        let mut data = alloc::vec![0u32; cursor];
        for &n in &order {
            let to = new_offset[n] as usize;
            let header = old[n];
            data[to] = header & OLD_CHILDREN;
            data[to + 1] = if header & OLD_ACCEPT != 0 {
                (ACCEPT_BIT >> 32) as u32
            } else {
                0
            };
            let degree = (header & OLD_CHILDREN).count_ones() as usize;
            for i in 0..degree {
                data[to + HEADER_WORDS + i] = new_offset[old[n + 1 + i] as usize];
            }
        }
        Ok(WordGraph {
            data,
            root: 0,
            node_count: order.len() as u32,
            arc_count: arcs,
            word_count,
        })
    }

    pub fn validate(&self) -> Result<(), GraphFormatError> {
        let len = self.data.len();
        if self.root as usize >= len {
            return Err(GraphFormatError::Corrupt);
        }

        let mut seen = 0usize;
        let mut stack = alloc::vec![self.root];
        let mut visited = alloc::vec![false; len];
        while let Some(node) = stack.pop() {
            let n = node as usize;
            if n >= len {
                return Err(GraphFormatError::Corrupt);
            }
            if visited[n] {
                continue;
            }
            visited[n] = true;
            seen += 1;
            if n + 1 >= len {
                return Err(GraphFormatError::Corrupt);
            }
            let k = (self.header_at(n) & CHILD_MASK).count_ones() as usize;
            if n + HEADER_WORDS - 1 + k >= len {
                return Err(GraphFormatError::Corrupt);
            }
            for i in 0..k {
                let target = self.data[n + HEADER_WORDS + i];
                if target as usize >= len {
                    return Err(GraphFormatError::Corrupt);
                }
                stack.push(target);
            }
        }
        if seen as u32 != self.node_count {
            return Err(GraphFormatError::Corrupt);
        }
        Ok(())
    }
}

impl fmt::Debug for WordGraph {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WordGraph")
            .field("nodes", &self.node_count)
            .field("arcs", &self.arc_count)
            .field("words", &self.word_count)
            .field("bytes", &self.size_bytes())
            .finish()
    }
}

#[derive(Clone, Debug)]
pub struct ArcIter {
    remaining: u64,
}

impl Iterator for ArcIter {
    type Item = u8;

    #[inline]
    fn next(&mut self) -> Option<u8> {
        if self.remaining == 0 {
            return None;
        }
        let letter = self.remaining.trailing_zeros() as u8;
        self.remaining &= self.remaining - 1;
        Some(letter)
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        let n = self.remaining.count_ones() as usize;
        (n, Some(n))
    }
}

impl ExactSizeIterator for ArcIter {}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GraphFormatError {
    BadMagic,
    Truncated,
    Corrupt,
}

impl fmt::Display for GraphFormatError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            GraphFormatError::BadMagic => write!(f, "not a scrabble word graph"),
            GraphFormatError::Truncated => write!(f, "word graph data is truncated"),
            GraphFormatError::Corrupt => write!(f, "word graph structure is inconsistent"),
        }
    }
}

#[cfg(feature = "std")]
impl std::error::Error for GraphFormatError {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexicon::GraphBuilder;

    fn graph_of(words: &[&str]) -> WordGraph {
        let mut b = GraphBuilder::new();
        let mut seqs: Vec<Vec<u8>> = words
            .iter()
            .map(|w| w.bytes().map(|c| c - b'A').collect())
            .collect();
        seqs.sort();
        for s in &seqs {
            b.insert(s).unwrap();
        }
        b.finish()
    }

    #[test]
    fn empty_graph_accepts_nothing() {
        let g = WordGraph::empty();
        assert!(!g.accepts(g.root()));
        assert_eq!(g.children(g.root()), 0);
        assert_eq!(g.word_count(), 0);
        assert!(g.collect_words().is_empty());
    }

    #[test]
    fn round_trips_every_inserted_word() {
        let words = ["AT", "ATE", "CAT", "CATS", "EAT", "TEA"];
        let g = graph_of(&words);
        assert_eq!(g.word_count(), words.len() as u64);
        for w in words {
            let seq: Vec<u8> = w.bytes().map(|c| c - b'A').collect();
            assert!(g.accepts_sequence(seq), "{w} should be accepted");
        }
        for w in ["A", "C", "CA", "CATT", "ZZZ", ""] {
            let seq: Vec<u8> = w.bytes().map(|c| c - b'A').collect();
            assert!(!g.accepts_sequence(seq), "{w:?} should be rejected");
        }
    }

    #[test]
    fn collect_words_returns_lexicographic_order() {
        let g = graph_of(&["CAT", "AT", "CATS", "ATE"]);
        let got: Vec<Vec<u8>> = g.collect_words();
        let want: Vec<Vec<u8>> = ["AT", "ATE", "CAT", "CATS"]
            .iter()
            .map(|w| w.bytes().map(|c| c - b'A').collect())
            .collect();
        assert_eq!(got, want);
    }

    #[test]
    fn child_and_child_unchecked_agree() {
        let g = graph_of(&["AT", "ATE", "CAT", "CATS"]);
        let root = g.root();
        for letter in 0..26u8 {
            match g.child(root, letter) {
                Some(n) => assert_eq!(n, g.child_unchecked(root, letter)),
                None => assert_eq!(g.children(root) & (1 << letter), 0),
            }
        }
    }

    #[test]
    fn arcs_iterates_exactly_the_child_mask() {
        let g = graph_of(&["AT", "BE", "CAT", "DO", "EAT"]);
        let root = g.root();
        let letters: Vec<u8> = g.arcs(root).collect();
        assert_eq!(letters, alloc::vec![0, 1, 2, 3, 4], "A B C D E");
        assert_eq!(letters.len(), g.degree(root) as usize);
        assert_eq!(g.arcs(root).len(), letters.len());
    }

    #[test]
    fn minimisation_shares_common_suffixes() {
        let shared = graph_of(&["CREATION", "LOCATION", "STATION", "VACATION"]);
        let unshared = graph_of(&["CREATIONX", "LOCATIONY", "STATIONZ", "VACATIONW"]);
        assert!(
            shared.node_count() < unshared.node_count(),
            "shared suffixes ({}) should minimise better than distinct ones ({})",
            shared.node_count(),
            unshared.node_count()
        );
    }

    #[test]
    fn bytes_round_trip() {
        let g = graph_of(&["AT", "ATE", "CAT", "CATS", "EAT"]);
        let bytes = g.to_bytes();
        let back = WordGraph::from_bytes(&bytes).expect("round trip should parse");
        assert_eq!(back, g);
        assert_eq!(back.collect_words(), g.collect_words());
        back.validate().expect("a round-tripped graph is valid");
    }

    #[test]
    fn from_bytes_rejects_bad_input() {
        assert_eq!(WordGraph::from_bytes(&[]), Err(GraphFormatError::Truncated));
        assert_eq!(
            WordGraph::from_bytes(&[0u8; 32]),
            Err(GraphFormatError::BadMagic)
        );
        let g = graph_of(&["AT"]);
        let mut bytes = g.to_bytes();
        bytes.truncate(bytes.len() - 4);
        assert_eq!(
            WordGraph::from_bytes(&bytes),
            Err(GraphFormatError::Truncated)
        );
    }

    #[test]
    fn validate_catches_an_out_of_range_target() {
        let g = graph_of(&["AT", "CAT"]);
        let mut data = g.raw().to_vec();

        data[g.root() as usize + 2] = 9_999;
        let broken = WordGraph::from_parts(
            data,
            g.root(),
            g.node_count(),
            g.arc_count(),
            g.word_count(),
        );
        assert_eq!(broken.validate(), Err(GraphFormatError::Corrupt));
    }

    #[test]
    fn header_split_matches_accessors() {
        let g = graph_of(&["AT", "ATE"]);
        let node = g.child(g.root(), 0).and_then(|n| g.child(n, 19)).unwrap();
        let (accepts, children) = WordGraph::split_header(g.header(node));
        assert_eq!(accepts, g.accepts(node));
        assert_eq!(children, g.children(node));
        assert!(accepts, "AT is a word");
        assert_ne!(children, 0, "ATE extends it");
    }

    fn seqs_graph(seqs: &[Vec<u8>]) -> WordGraph {
        let mut sorted = seqs.to_vec();
        sorted.sort();
        sorted.dedup();
        let mut b = GraphBuilder::new();
        for s in &sorted {
            b.insert(s).unwrap();
        }
        b.finish()
    }

    #[test]
    fn letters_above_the_old_31_limit_work_in_every_accessor() {
        let top = (MAX_LETTERS - 1) as u8;
        let seqs: Vec<Vec<u8>> = alloc::vec![
            alloc::vec![0, 31],
            alloc::vec![0, 32, 33],
            alloc::vec![31],
            alloc::vec![40, top],
            alloc::vec![top, 0, top],
            alloc::vec![33, 5, 62],
        ];
        let g = seqs_graph(&seqs);
        g.validate().unwrap();
        for s in &seqs {
            assert!(g.accepts_sequence(s.iter().copied()), "{s:?} is missing");
        }
        for s in [
            alloc::vec![32u8],
            alloc::vec![0, 33],
            alloc::vec![top],
            alloc::vec![40],
        ] {
            assert!(
                !g.accepts_sequence(s.iter().copied()),
                "{s:?} should be rejected"
            );
        }
        let root = g.root();
        let letters: Vec<u8> = g.arcs(root).collect();
        assert_eq!(letters, alloc::vec![0, 31, 33, 40, top]);
        for &l in &letters {
            assert_eq!(g.child(root, l), Some(g.child_unchecked(root, l)));
        }
        assert_eq!(g.child(root, 32), None);
        assert_eq!(g.degree(root), 5);

        let mut want = seqs.clone();
        want.sort();
        assert_eq!(g.collect_words(), want);

        let bytes = g.to_bytes();
        assert_eq!(&bytes[..4], &WordGraph::MAGIC);
        let back = WordGraph::from_bytes(&bytes).unwrap();
        assert_eq!(back, g);
        back.validate().unwrap();
    }

    #[test]
    fn the_accept_flag_never_collides_with_the_highest_letter() {
        let top = (MAX_LETTERS - 1) as u8;
        let g = seqs_graph(&[alloc::vec![top]]);
        let (accepts_root, children) = WordGraph::split_header(g.header(g.root()));
        assert!(!accepts_root, "the root does not accept the empty word");
        assert_eq!(children, 1u64 << top);
        let leaf = g.child(g.root(), top).unwrap();
        assert!(g.accepts(leaf));
        assert_eq!(g.children(leaf), 0);
    }

    /// Writes a graph the way upstream scrabble 0.1.0 did (`SWG1`, one 32-bit
    /// header word per node), so the legacy reader can be checked against it.
    fn v1_bytes(g: &WordGraph) -> Vec<u8> {
        let raw = g.raw();
        let mut old: Vec<u32> = Vec::new();
        let mut at = 0usize;
        let mut map = alloc::collections::BTreeMap::new();
        let mut nodes = Vec::new();
        while at < raw.len() {
            let lo = raw[at];
            let hi = raw[at + 1];
            assert_eq!(lo >> 31, 0, "v1 can only hold letters 0..=30");
            let degree = lo.count_ones() as usize;
            map.insert(at as u32, old.len() as u32);
            nodes.push((at, degree));
            old.push(lo | (hi & 0x8000_0000));
            old.extend(core::iter::repeat_n(0, degree));
            at += 2 + degree;
        }
        let mut cursor = 0usize;
        for &(at, degree) in &nodes {
            for i in 0..degree {
                old[cursor + 1 + i] = map[&raw[at + 2 + i]];
            }
            cursor += 1 + degree;
        }
        let mut out = Vec::new();
        out.extend_from_slice(&WordGraph::MAGIC_V1);
        out.extend_from_slice(&g.node_count().to_le_bytes());
        out.extend_from_slice(&g.arc_count().to_le_bytes());
        out.extend_from_slice(&(old.len() as u32).to_le_bytes());
        out.extend_from_slice(&map[&g.root()].to_le_bytes());
        out.extend_from_slice(&g.word_count().to_le_bytes());
        for w in old {
            out.extend_from_slice(&w.to_le_bytes());
        }
        out
    }

    #[test]
    fn upstream_v1_graphs_load_and_are_widened_exactly() {
        let g = graph_of(&[
            "AT", "ATE", "CAT", "CATS", "EAT", "EATS", "SAT", "SATE", "TA", "TAE",
        ]);
        let v1 = v1_bytes(&g);
        assert_eq!(&v1[..4], b"SWG1");
        let back = WordGraph::from_bytes(&v1).expect("v1 must still load");
        assert_eq!(back, g, "widening must reproduce the breadth-first layout");
        assert_eq!(back.collect_words(), g.collect_words());
        back.validate().unwrap();

        let mut cut = v1.clone();
        cut.truncate(cut.len() - 4);
        assert_eq!(
            WordGraph::from_bytes(&cut),
            Err(GraphFormatError::Truncated)
        );

        let mut bad = v1.clone();
        let last = bad.len() - 4;
        bad[last..].copy_from_slice(&u32::MAX.to_le_bytes());
        // the last word is an arc target or a header; either way the loader
        // must reject or accept without panicking
        let _ = WordGraph::from_bytes(&bad);
    }
}
