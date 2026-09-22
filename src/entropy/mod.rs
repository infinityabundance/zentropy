//! Entropy backends.
//!
//! Zentropy does not force one coder onto every stream (§17). Prediction-heavy
//! main text will favour a binary range/arithmetic coder driven by a context
//! mixer; finite side streams may favour rANS/FSE; tiny alphabets may favour a
//! specialised code. A stream chooses its coder using actual emitted bytes,
//! including any model or table description it needs.
//!
//! At this layer we provide two well-understood primitives:
//!
//! * a carryless binary range coder (the classic 32-bit "[x1, x2]" coder used
//!   throughout the PAQ/lpaq/zpaq lineage), and
//! * a byte-interleaved rANS coder for finite-alphabet side streams.
//!
//! Both are specified exactly and covered by round-trip tests.

pub mod rans;

/// Number of bits of probability precision used by the binary range coder.
/// 12 bits is the PAQ-family convention: accurate enough that quantisation is
/// invisible, cheap enough to keep the hot loop integer-only.
pub const PROB_BITS: u32 = 12;
/// Denominator of a probability: `P(1) = p / 4096`.
pub const PROB_SCALE: u32 = 1 << PROB_BITS;

/// Binary range encoder.
///
/// Invariant: `x1 <= x2` and `x1 ^ x2` has a nonzero top byte before each
/// symbol is coded. After coding, a common top byte is shifted out.
#[derive(Debug, Clone)]
pub struct RangeEncoder {
    x1: u32,
    x2: u32,
    out: Vec<u8>,
    /// Phase 10: accumulated *ideal* code length in bits, for the modelling
    /// experiments that must price a representation by what the coder actually
    /// charges rather than by a raw byte count. Research-only (`vocab-price`):
    /// the field and its arithmetic are compiled out of the scored build, so it
    /// cannot cost `S` a byte.
    #[cfg(feature = "vocab-price")]
    cost: f64,
    /// Phase 14.1: the same ideal cost, partitioned by a caller-supplied class
    /// index. Empty unless a caller installs one; each coded decision is charged
    /// to `class` after the caller declares it with [`RangeEncoder::set_class`].
    /// This is a *bookkeeping* accumulator: it never changes a coded bit, so the
    /// coder's output is bit-identical with and without it. Research-only for the
    /// same reason as `cost` — an `f64` must never reach a scored artifact.
    #[cfg(feature = "vocab-price")]
    class_cost: Vec<f64>,
    /// Phase 14.1: the class the next coded decision is charged to.
    #[cfg(feature = "vocab-price")]
    class: usize,
}

impl Default for RangeEncoder {
    fn default() -> Self {
        Self::new()
    }
}

impl RangeEncoder {
    pub fn new() -> Self {
        RangeEncoder {
            x1: 0,
            x2: 0xffff_ffff,
            out: Vec::new(),
            #[cfg(feature = "vocab-price")]
            cost: 0.0,
            #[cfg(feature = "vocab-price")]
            class_cost: Vec::new(),
            #[cfg(feature = "vocab-price")]
            class: 0,
        }
    }

    pub fn with_capacity(n: usize) -> Self {
        RangeEncoder {
            x1: 0,
            x2: 0xffff_ffff,
            out: Vec::with_capacity(n),
            #[cfg(feature = "vocab-price")]
            cost: 0.0,
            #[cfg(feature = "vocab-price")]
            class_cost: Vec::new(),
            #[cfg(feature = "vocab-price")]
            class: 0,
        }
    }

    /// Phase 10: the ideal code length produced so far, in bits.
    ///
    /// This is Shannon cost, so it is a *diagnostic*: it prices a representation
    /// for a search, and never decides an adoption (`S` does that). It is also
    /// what makes the vocabulary experiment possible — a word's literal cost
    /// under the real model is precisely the quantity a raw-byte heuristic
    /// cannot see.
    #[cfg(feature = "vocab-price")]
    #[inline]
    pub fn cost_bits(&self) -> f64 {
        self.cost
    }

    /// Phase 14.1: start partitioning every subsequent coded decision across `n`
    /// classes. Idempotent; calling it again resets the accumulators.
    #[cfg(feature = "vocab-price")]
    pub fn set_class_count(&mut self, n: usize) {
        self.class_cost = vec![0.0; n];
        self.class = 0;
    }

    /// Phase 14.1: charge the next coded decision to class `i`. Out-of-range
    /// indices are ignored by the accumulator rather than panicking, so a
    /// mislabelled class cannot abort a measurement pass.
    #[cfg(feature = "vocab-price")]
    #[inline]
    pub fn set_class(&mut self, i: usize) {
        self.class = i;
    }

    /// Phase 14.1: ideal code length charged to class `i`, in bits.
    #[cfg(feature = "vocab-price")]
    #[inline]
    pub fn class_cost(&self, i: usize) -> f64 {
        self.class_cost.get(i).copied().unwrap_or(0.0)
    }

    /// Phase 14.1: the whole per-class partition, in bits.
    #[cfg(feature = "vocab-price")]
    #[inline]
    pub fn class_costs(&self) -> &[f64] {
        &self.class_cost
    }

    /// Code one bit with `p = P(bit = 1)` in `[1, 4095]`.
    ///
    /// Passing `p = 0` or `p = 4096` is a programming error: it would make the
    /// interval degenerate. We clamp defensively so a numerical slip cannot
    /// desynchronise a decoder, but tests assert sane inputs.
    #[inline]
    pub fn encode(&mut self, bit: u32, p: u32) {
        debug_assert!((1..PROB_SCALE).contains(&p), "p={p} out of range");
        let p = p.clamp(1, PROB_SCALE - 1);
        #[cfg(feature = "vocab-price")]
        {
            // Ideal length of this binary decision: -log2 of the probability the
            // model assigned to the outcome that actually occurred.
            let pi = if bit != 0 { p } else { PROB_SCALE - p };
            let d = -(pi as f64 / PROB_SCALE as f64).log2();
            self.cost += d;
            // Phase 14.1: the same quantity, also charged to the caller's class.
            // `get_mut` keeps a default (uninstalled) accumulator free.
            if let Some(slot) = self.class_cost.get_mut(self.class) {
                *slot += d;
            }
        }
        let range = self.x2 - self.x1;
        let xmid = self.x1 + ((range >> PROB_BITS) * p);
        if bit != 0 {
            self.x2 = xmid;
        } else {
            self.x1 = xmid + 1;
        }
        while (self.x1 ^ self.x2) & 0xff00_0000 == 0 {
            self.out.push((self.x2 >> 24) as u8);
            self.x1 <<= 8;
            self.x2 = (self.x2 << 8) | 0xff;
        }
    }

    /// Flush the coder. Writes four bytes so a decoder that reads past the end
    /// of the stream can always reconstruct `x` exactly; the resulting archive
    /// length is defined by the container, not by the coder.
    pub fn finish(mut self) -> Vec<u8> {
        // Any value in [x1, x2] identifies the final interval. Writing x1 in
        // full is simplest and unambiguous.
        for shift in [24, 16, 8, 0] {
            self.out.push((self.x1 >> shift) as u8);
        }
        self.out
    }
}

/// Binary range decoder, the exact inverse of [`RangeEncoder`].
#[derive(Debug, Clone)]
pub struct RangeDecoder<'a> {
    x1: u32,
    x2: u32,
    x: u32,
    input: &'a [u8],
    pos: usize,
}

impl<'a> RangeDecoder<'a> {
    /// Begin decoding. The first four input bytes initialise `x`; reads past
    /// the end yield zero, which matches the encoder's flush.
    pub fn new(input: &'a [u8]) -> Self {
        let mut d = RangeDecoder {
            x1: 0,
            x2: 0xffff_ffff,
            x: 0,
            input,
            pos: 0,
        };
        for _ in 0..4 {
            d.x = (d.x << 8) | d.next_byte() as u32;
        }
        d
    }

    #[inline]
    fn next_byte(&mut self) -> u8 {
        let b = self.input.get(self.pos).copied().unwrap_or(0);
        self.pos += 1;
        b
    }

    /// Decode one bit given `p = P(bit = 1)` in `[1, 4095]`.
    #[inline]
    pub fn decode(&mut self, p: u32) -> u32 {
        let p = p.clamp(1, PROB_SCALE - 1);
        let range = self.x2 - self.x1;
        let xmid = self.x1 + ((range >> PROB_BITS) * p);
        let bit = if self.x <= xmid { 1 } else { 0 };
        if bit != 0 {
            self.x2 = xmid;
        } else {
            self.x1 = xmid + 1;
        }
        while (self.x1 ^ self.x2) & 0xff00_0000 == 0 {
            self.x1 <<= 8;
            self.x2 = (self.x2 << 8) | 0xff;
            self.x = (self.x << 8) | self.next_byte() as u32;
        }
        bit
    }

    /// Bytes consumed so far (useful for pricing experiments).
    pub fn consumed(&self) -> usize {
        self.pos
    }
}

/// An adaptive binary probability, the smallest useful model.
///
/// Stored at 16-bit resolution and updated toward the observed bit with a
/// power-of-two rate. This is used only by floors and tests; the real model
/// uses the context-mixing stack.
#[derive(Debug, Clone)]
pub struct AdaptiveBit {
    /// `P(1)` scaled to 16 bits.
    pub p: u16,
    /// Adaptation shift; larger is slower.
    pub rate: u32,
}

impl AdaptiveBit {
    pub fn new(rate: u32) -> Self {
        AdaptiveBit { p: 32768, rate }
    }

    /// Return a 12-bit probability for the coder.
    #[inline]
    pub fn predict(&self) -> u32 {
        (self.p as u32 >> 4).clamp(1, PROB_SCALE - 1)
    }

    #[inline]
    pub fn update(&mut self, bit: u32) {
        let target: i32 = if bit != 0 { 65535 } else { 0 };
        let p = self.p as i32;
        self.p = (p + ((target - p) >> self.rate)) as u16;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn random_bits(n: usize, seed: u64) -> Vec<(u32, u32)> {
        // Deterministic xorshift so the test is reproducible.
        let mut s = seed | 1;
        let mut out = Vec::with_capacity(n);
        for _ in 0..n {
            s ^= s << 13;
            s ^= s >> 7;
            s ^= s << 17;
            let bit = (s & 1) as u32;
            // A deliberately lopsided, varying probability.
            let p = 200 + ((s >> 8) % 3600) as u32;
            out.push((bit, p));
        }
        out
    }

    #[test]
    fn roundtrip_random() {
        for seed in 1..8u64 {
            let bits = random_bits(20_000, seed * 0x9E37_79B9);
            let mut enc = RangeEncoder::new();
            for &(b, p) in &bits {
                enc.encode(b, p);
            }
            let bytes = enc.finish();
            let mut dec = RangeDecoder::new(&bytes);
            for (i, &(b, p)) in bits.iter().enumerate() {
                assert_eq!(dec.decode(p), b, "seed={seed} i={i}");
            }
        }
    }

    #[test]
    fn roundtrip_extreme_probabilities() {
        let bits = [(1, 1), (0, 4094), (1, 2), (0, 1), (1, 4095), (0, 4095)];
        let mut enc = RangeEncoder::new();
        for &(b, p) in &bits {
            enc.encode(b, p);
        }
        let bytes = enc.finish();
        let mut dec = RangeDecoder::new(&bytes);
        for &(b, p) in &bits {
            assert_eq!(dec.decode(p), b);
        }
    }

    #[test]
    fn adaptive_bit_converges() {
        let mut m = AdaptiveBit::new(4);
        for _ in 0..10_000 {
            m.update(1);
        }
        assert!(m.predict() > 4000);
        for _ in 0..10_000 {
            m.update(0);
        }
        assert!(m.predict() < 100);
    }

    #[test]
    fn compressed_size_reflects_entropy() {
        // A strongly skewed sequence must be much smaller than a uniform one.
        let mut enc = RangeEncoder::new();
        for _ in 0..10_000 {
            enc.encode(1, 4000);
        }
        let skewed = enc.finish().len();

        let mut enc = RangeEncoder::new();
        for i in 0..10_000 {
            enc.encode((i & 1) as u32, 2048);
        }
        let uniform = enc.finish().len();
        assert!(skewed < uniform / 4, "skewed={skewed} uniform={uniform}");
    }
}
