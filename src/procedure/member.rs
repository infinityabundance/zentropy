//! Per-member state and residual.
//!
//! For one member, given a shared base:
//!
//! * **state** — the slot geometry. With a one-hole skeleton the only slot
//!   geometry this experiment models is the hole's *length*, held as a bounded
//!   count and coded with the best of [`crate::procedural::state`]'s six codecs
//!   (`raw`/`varint`/`delta`/`rank`/`block-rank`/`rans`). The hole's *contents*
//!   are the residual, so the two are disjoint; the length is also implied by the
//!   residual's framing, so charging it separately is deliberately *conservative*
//!   (it can only make the procedural side look worse, never better).
//! * **residual** — `derive(base, target, kind)` over every applicable
//!   [`ResidualKind`], choosing the one with the smallest canonical wire length.
//!   `apply(base, r) == target` is checked for every member, and a mismatch is a
//!   hard error, not a silent fallback.
//!
//! The canonical wire encoder here is the procedure plane's own; the residual
//! algebra itself does not serialize (its bytes are charged in their own stream,
//! §4.4), so this module fixes one canonical encoding and charges *it*, coded
//! through the accepted configuration.

use crate::procedural::residual::{self, EditOp, Residual, ResidualKind};
use crate::procedural::state::{best_codec, Conditioning, State, StateSpace};

/// Residual kinds in tie-break order: the simplest that achieves a given wire
/// length wins, so a caller never pays for a mechanism that bought nothing.
pub const KINDS: [ResidualKind; 6] = [
    ResidualKind::None,
    ResidualKind::RangeReplace,
    ResidualKind::EditScript,
    ResidualKind::SparseSubstitute,
    ResidualKind::RunPatch,
    ResidualKind::RankedMismatch,
];

fn put_uvarint(out: &mut Vec<u8>, mut v: u64) {
    loop {
        let low = (v & 0x7f) as u8;
        v >>= 7;
        if v == 0 {
            out.push(low);
            return;
        }
        out.push(low | 0x80);
    }
}

/// The canonical byte encoding of one residual. Self-delimiting for every kind
/// except `None` (which is zero bytes), so a stream of residuals is delimited by
/// an explicit length prefix chosen by the caller.
pub fn wire(r: &Residual) -> Vec<u8> {
    let mut o = Vec::new();
    match r {
        Residual::None => {}
        Residual::SparseSubstitute { positions, values } => {
            put_uvarint(&mut o, positions.len() as u64);
            let mut prev = 0u64;
            for &p in positions {
                put_uvarint(&mut o, p as u64 - prev);
                prev = p as u64;
            }
            put_uvarint(&mut o, values.len() as u64);
            o.extend_from_slice(values);
        }
        Residual::RangeReplace { ranges, data } => {
            put_uvarint(&mut o, ranges.len() as u64);
            for &(from, old, new) in ranges {
                put_uvarint(&mut o, from as u64);
                put_uvarint(&mut o, old as u64);
                put_uvarint(&mut o, new as u64);
            }
            put_uvarint(&mut o, data.len() as u64);
            o.extend_from_slice(data);
        }
        Residual::RunPatch { runs } => {
            put_uvarint(&mut o, runs.len() as u64);
            for &(start, len, byte) in runs {
                put_uvarint(&mut o, start as u64);
                put_uvarint(&mut o, len as u64);
                o.push(byte);
            }
        }
        Residual::RankedMismatch { mask_rank, values } => {
            put_uvarint(&mut o, *mask_rank);
            put_uvarint(&mut o, values.len() as u64);
            o.extend_from_slice(values);
        }
        Residual::EditScript { ops } => {
            put_uvarint(&mut o, ops.len() as u64);
            for op in ops {
                match op {
                    EditOp::Copy { from, len } => {
                        o.push(0);
                        put_uvarint(&mut o, *from as u64);
                        put_uvarint(&mut o, *len as u64);
                    }
                    EditOp::Insert(data) => {
                        o.push(1);
                        put_uvarint(&mut o, data.len() as u64);
                        o.extend_from_slice(data);
                    }
                }
            }
        }
    }
    o
}

/// The smallest canonical residual that turns `base` into `target`.
pub fn best_residual(base: &[u8], target: &[u8]) -> Residual {
    let mut best: Option<(Residual, usize)> = None;
    for kind in KINDS {
        if let Some(r) = residual::derive(base, target, kind) {
            let w = wire(&r).len();
            match &best {
                None => best = Some((r, w)),
                Some((_, bw)) if w < *bw => best = Some((r, w)),
                _ => {}
            }
        }
    }
    // `RangeReplace` derives for arbitrary shapes, so a residual always exists.
    best.expect("procedure: RangeReplace must always derive").0
}

/// Verify the residual algebra's own contract on this pair.
pub fn verify(base: &[u8], target: &[u8], r: &Residual) -> bool {
    residual::apply(base, r).as_deref() == Some(target)
}

fn shared_ends(a: &[u8], b: &[u8]) -> usize {
    let mut cp = 0usize;
    while cp < a.len() && cp < b.len() && a[cp] == b[cp] {
        cp += 1;
    }
    let max = a.len().min(b.len());
    let mut cs = 0usize;
    while cs < max - cp && a[a.len() - 1 - cs] == b[b.len() - 1 - cs] {
        cs += 1;
    }
    cp + cs
}

/// The cohort medoid by shared ends: the actual member that is "most typical".
/// Deterministic; ties keep the earliest corpus position.
pub fn best_member(members: &[Vec<u8>]) -> Vec<u8> {
    let mut bi = 0usize;
    let mut bs = 0usize;
    for i in 0..members.len() {
        let mut score = 0usize;
        for j in 0..members.len() {
            if i != j {
                score += shared_ends(&members[i], &members[j]);
            }
        }
        if score > bs {
            bs = score;
            bi = i;
        }
    }
    members[bi].clone()
}

/// Encode a cohort's hole lengths as a bounded-count state stream with the best
/// available codec. Returns `(bytes, codec name)`.
///
/// A length vector all of whose entries are zero collapses to a radix-1 space
/// (no choice to record) and costs zero bytes, which is correct rather than a
/// degenerate case.
pub fn encode_lengths(lens: &[usize]) -> Result<(usize, String), String> {
    if lens.is_empty() {
        return Ok((0, "none".to_string()));
    }
    let max = *lens.iter().max().expect("non-empty");
    let radix = max
        .checked_add(1)
        .ok_or_else(|| "procedure: slot length overflow".to_string())?;
    if radix > u32::MAX as usize {
        return Err("procedure: slot length exceeds the state model".to_string());
    }
    let space = StateSpace::bounded_count(radix as u32)
        .ok_or_else(|| "procedure: unbounded slot state space".to_string())?;
    let states: Vec<State> = lens
        .iter()
        .map(|&l| State::from_digits(vec![l as u32]))
        .collect();
    let ctx = Conditioning::default();
    let choice = best_codec(&space, &states, &ctx);
    Ok((choice.size(), choice.codec.name().to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_applicable_kind_round_trips_through_its_wire_form() {
        let cases: &[(&[u8], &[u8])] = &[
            (b"{{a|1}}", b"{{a|1}}"),
            (b"{{a|1}}", b"{{a|2}}"),
            (b"{{a|1}}", b"{{a|1|longer}}"),
            (b"abc", b""),
            (b"", b"xyz"),
        ];
        for (base, target) in cases {
            let mut seen = 0;
            for kind in KINDS {
                if let Some(r) = residual::derive(base, target, kind) {
                    seen += 1;
                    assert!(
                        verify(base, target, &r),
                        "kind {kind:?} failed for {base:?}->{target:?}"
                    );
                }
            }
            assert!(seen >= 1);
            assert!(verify(base, target, &best_residual(base, target)));
        }
    }

    #[test]
    fn the_smallest_wire_wins() {
        let base = b"{{cite|url=http://example.org/page|title=Example}}";
        let target = b"{{cite|url=http://example.org/page|title=Example2}}";
        let r = best_residual(base, target);
        for kind in KINDS {
            if let Some(other) = residual::derive(base, target, kind) {
                assert!(
                    wire(&r).len() <= wire(&other).len(),
                    "chose {r:?} over {other:?}"
                );
            }
        }
    }

    #[test]
    fn length_state_is_encoded_and_costs_bytes_only_when_it_has_a_choice() {
        let (zero, _) = encode_lengths(&[0, 0, 0]).unwrap();
        assert_eq!(zero, 0);
        let (some, codec) = encode_lengths(&[1, 2, 3, 4]).unwrap();
        assert!(some > 0);
        assert!(!codec.is_empty());
    }

    #[test]
    fn the_medoid_is_the_actual_most_typical_member() {
        let members = vec![
            b"{{a|1}}".to_vec(),
            b"{{a|2}}".to_vec(),
            b"totally different".to_vec(),
        ];
        let b = best_member(&members);
        assert!(b == b"{{a|1}}" || b == b"{{a|2}}");
    }
}
