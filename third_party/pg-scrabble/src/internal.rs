#[inline]
pub(crate) fn bits(mask: u32) -> BitIter {
    BitIter { remaining: mask }
}

pub(crate) struct BitIter {
    remaining: u32,
}

impl Iterator for BitIter {
    type Item = u8;

    #[inline]
    fn next(&mut self) -> Option<u8> {
        if self.remaining == 0 {
            return None;
        }
        let b = self.remaining.trailing_zeros() as u8;
        self.remaining &= self.remaining - 1;
        Some(b)
    }
}
