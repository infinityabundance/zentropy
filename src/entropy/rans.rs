//! Byte-interleaved rANS for finite-alphabet side streams.
//!
//! This is an *optional* backend (§17). The main text is expected to be coded
//! by the mix-driven range coder; rANS earns its place only when a side stream
//! has a small, stable alphabet and a table whose description is cheaper than
//! the coding gain. The implementation uses exact division rather than the
//! reciprocal table trick: correctness and auditability first, and the table
//! description cost is measured anyway.
//!
//! Contract (pinned): `decode(encode(symbols, table), table, symbols.len())`
//! returns `symbols` exactly. The stream is self-delimiting given `n`, so `n`
//! must be stored by the caller in the container.
//!
//! Reference: J. Duda, "Asymmetric numeral systems"; F. Giesen, `rans_byte.h`.

/// Lower bound of the rANS state. `2^23` is the standard byte-interleaved rANS
/// choice: it guarantees renormalisation can always emit whole bytes.
pub const RANS_L: u32 = 1 << 23;

/// Number of bits of frequency precision. Frequencies sum to `1 << SCALE_BITS`.
pub const SCALE_BITS: u32 = 12;

/// A normalised frequency table over a small alphabet.
#[derive(Debug, Clone)]
pub struct Table {
    /// `freq[s]`, sum = 1<<SCALE_BITS. Zero means the symbol is impossible.
    pub freq: Vec<u32>,
    /// `cum[s]` = sum of `freq[0..s]`.
    pub cum: Vec<u32>,
    /// `slot_to_sym[slot]` for O(1) decoding.
    slot_to_sym: Vec<u16>,
}

impl Table {
    /// Build a table from raw non-negative counts. Zero-count symbols get
    /// frequency zero and can never be coded; decoding a slot that maps to a
    /// zero-frequency symbol is impossible by construction.
    ///
    /// Normalisation guarantees every symbol that *can* occur has frequency at
    /// least 1, provided the alphabet fits in the scale.
    pub fn from_counts(counts: &[u32]) -> Table {
        let m = 1u64 << SCALE_BITS;
        assert!(
            counts.len() <= m as usize,
            "alphabet {} exceeds rANS scale 2^{}",
            counts.len(),
            SCALE_BITS
        );
        let total: u64 = counts.iter().map(|&c| c as u64).sum();
        let mut freq = vec![0u32; counts.len()];
        if total == 0 {
            // Degenerate: spread uniformly over the whole alphabet.
            let k = counts.len() as u64;
            for (i, f) in freq.iter_mut().enumerate() {
                *f = ((m * (i as u64 + 1) / k) - (m * i as u64 / k)) as u32;
            }
        } else {
            // Largest-remainder-ish allocation, then fix rounding so the sum is
            // exactly m and no present symbol gets zero.
            let mut assigned = 0u64;
            for (i, &c) in counts.iter().enumerate() {
                if c == 0 {
                    continue;
                }
                let f = ((c as u64 * m) / total).max(1);
                freq[i] = f as u32;
                assigned += f;
            }
            // Adjust the largest entry to make the total exact.
            if assigned != m {
                let idx = freq
                    .iter()
                    .enumerate()
                    .max_by_key(|(_, &f)| f)
                    .map(|(i, _)| i)
                    .unwrap();
                let delta = m as i64 - assigned as i64;
                let nf = freq[idx] as i64 + delta;
                assert!(nf >= 1, "rANS normalisation underflow");
                freq[idx] = nf as u32;
            }
        }
        let mut cum = vec![0u32; counts.len()];
        let mut acc = 0u32;
        for i in 0..counts.len() {
            cum[i] = acc;
            acc += freq[i];
        }
        assert_eq!(acc as u64, m, "frequency sum must equal 1<<SCALE_BITS");

        let mut slot_to_sym = vec![0u16; m as usize];
        for (s, f) in freq.iter().enumerate() {
            for slot in cum[s]..cum[s] + f {
                slot_to_sym[slot as usize] = s as u16;
            }
        }
        Table {
            freq,
            cum,
            slot_to_sym,
        }
    }

    #[inline]
    fn sym(&self, s: u16) -> (u32, u32) {
        (self.freq[s as usize], self.cum[s as usize])
    }
}

/// Encode `symbols` with `table`, returning a self-contained stream.
///
/// Symbols are encoded in reverse and the byte buffer is reversed at the end,
/// so the decoder reads forward and recovers the original order.
pub fn encode(symbols: &[u16], table: &Table) -> Vec<u8> {
    let mut state = RANS_L;
    let mut buf: Vec<u8> = Vec::with_capacity(symbols.len() * 2 + 4);
    for &s in symbols.iter().rev() {
        let (f, c) = table.sym(s);
        debug_assert!(f > 0, "cannot encode a zero-frequency symbol");
        let x_max = (((RANS_L as u64) >> SCALE_BITS) << 8) * f as u64;
        while state as u64 >= x_max {
            buf.push((state & 0xff) as u8);
            state >>= 8;
        }
        state = ((state / f) << SCALE_BITS) + (state % f) + c;
    }
    // The whole buffer is reversed at the end, so the state must be written
    // big-endian here to end up little-endian at the head of the stream.
    buf.extend_from_slice(&state.to_be_bytes());
    buf.reverse();
    buf
}

/// rANS decoder.
pub struct Decoder<'a> {
    state: u32,
    input: &'a [u8],
    pos: usize,
}

impl<'a> Decoder<'a> {
    /// Initialise from the head of `input`.
    pub fn new(input: &'a [u8]) -> Option<Self> {
        if input.len() < 4 {
            return None;
        }
        let mut state = 0u32;
        for i in 0..4 {
            state |= (input[i] as u32) << (8 * i);
        }
        if state < RANS_L {
            return None;
        }
        Some(Decoder {
            state,
            input,
            pos: 4,
        })
    }

    /// Decode one symbol.
    #[inline]
    pub fn decode(&mut self, table: &Table) -> Option<u16> {
        let slot = self.state & ((1 << SCALE_BITS) - 1);
        let s = *table.slot_to_sym.get(slot as usize)?;
        let (f, c) = table.sym(s);
        if f == 0 {
            return None;
        }
        self.state = f * (self.state >> SCALE_BITS) + slot - c;
        while self.state < RANS_L {
            let b = self.input.get(self.pos).copied().unwrap_or(0);
            self.pos += 1;
            self.state = (self.state << 8) | b as u32;
        }
        Some(s)
    }

    /// Decode exactly `n` symbols into `out`.
    pub fn decode_n(&mut self, table: &Table, n: usize, out: &mut Vec<u16>) -> bool {
        out.clear();
        out.reserve(n);
        for _ in 0..n {
            match self.decode(table) {
                Some(s) => out.push(s),
                None => return false,
            }
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_skewed() {
        let counts = [1000u32, 500, 250, 10, 1, 0];
        let table = Table::from_counts(&counts);
        let mut symbols = Vec::new();
        let mut s = 12345u64;
        for _ in 0..50_000 {
            s ^= s << 13;
            s ^= s >> 7;
            s ^= s << 17;
            // Bias toward low symbols, which have high frequency.
            let sym = ((s % 100) as usize).min(4) as u16;
            symbols.push(sym);
        }
        let stream = encode(&symbols, &table);
        let mut dec = Decoder::new(&stream).expect("valid stream");
        let mut out = Vec::new();
        assert!(dec.decode_n(&table, symbols.len(), &mut out));
        assert_eq!(out, symbols);
    }

    #[test]
    fn roundtrip_uniform() {
        let counts = vec![1u32; 256];
        let table = Table::from_counts(&counts);
        let symbols: Vec<u16> = (0..10_000).map(|i| ((i * 37) % 256) as u16).collect();
        let stream = encode(&symbols, &table);
        let mut dec = Decoder::new(&stream).unwrap();
        let mut out = Vec::new();
        assert!(dec.decode_n(&table, symbols.len(), &mut out));
        assert_eq!(out, symbols);
    }

    #[test]
    fn roundtrip_single_symbol() {
        let table = Table::from_counts(&[1u32]);
        let symbols = vec![0u16; 1000];
        let stream = encode(&symbols, &table);
        // A constant stream should be very small.
        assert!(stream.len() < 20, "len={}", stream.len());
        let mut dec = Decoder::new(&stream).unwrap();
        let mut out = Vec::new();
        assert!(dec.decode_n(&table, symbols.len(), &mut out));
        assert_eq!(out, symbols);
    }

    #[test]
    fn table_sums_exactly() {
        for counts in [
            vec![1u32, 1, 1],
            vec![7, 3],
            vec![1000, 1],
            (0..300u32).map(|i| i + 1).collect::<Vec<_>>(),
        ] {
            let t = Table::from_counts(&counts);
            let sum: u32 = t.freq.iter().sum();
            assert_eq!(sum, 1 << SCALE_BITS, "counts={counts:?}");
            for (i, &c) in counts.iter().enumerate() {
                if c > 0 {
                    assert!(t.freq[i] >= 1, "zero freq for present symbol {i}");
                }
            }
        }
    }
}
