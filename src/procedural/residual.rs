//! Typed residual algebra (plan §4.4): recover a target from a base plus a small,
//! *typed* correction.
//!
//! A residual is the part of the target that the program could not explain. It is
//! deliberately not "the diff": each variant encodes a different *shape* of
//! innovation, and `WHERE` (which positions) is separated from `WHAT` (which
//! bytes), because the two have different statistics and a coder can exploit that
//! only if the shape is explicit. A token-level mismatch and a run-length fill are
//! different objects even when they touch the same positions.
//!
//! **Exactness.** `derive` and `apply` are inverse in the only direction that
//! matters: if `derive(base, target, kind)` returns `Some(r)` then
//! `apply(base, &r) == Some(target)`. When a residual cannot represent the change,
//! `derive` returns `None` rather than a residual that would silently corrupt the
//! reconstruction.
//!
//! **Decode path is integer-only.** `apply` is what a decoder runs; it validates
//! every field and returns `None` on anything malformed, so a forged residual is a
//! typed rejection rather than a panic or a wrong byte.

use super::rank::{rank_subset, unrank_subset};

/// Which residual family a caller wants. Kept separate from [`Residual`] so the
/// emitter's choice of shape is not confused with the residual's content.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResidualKind {
    /// The base is already the target.
    None,
    /// Same length; a few positions take new bytes.
    SparseSubstitute,
    /// Any change, as a sequence of replaced `(from, len)` ranges.
    RangeReplace,
    /// Same length; whole runs are overwritten with one repeated byte.
    RunPatch,
    /// Same length; mismatches addressed by the colex rank of their position set.
    RankedMismatch,
    /// Any change, as copy/insert edit operations.
    EditScript,
}

/// One operation of an [`Residual::EditScript`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EditOp {
    /// Copy `len` bytes of the base starting at `from`.
    Copy { from: u32, len: u32 },
    /// Append literal bytes.
    Insert(Vec<u8>),
}

/// A typed correction that turns a base into a target.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Residual {
    /// The base *is* the target; the residual carries no state.
    None,
    /// Same-length substitution at `positions`, taking the bytes from `values`.
    /// `positions` is strictly ascending, which is what makes the representation
    /// canonical and the rank of [`Residual::RankedMismatch`] well defined.
    SparseSubstitute {
        positions: Vec<u32>,
        values: Vec<u8>,
    },
    /// Replace the base range `ranges[i] = (from, old_len, new_len)`: the base bytes
    /// `[from, from+old_len)` are replaced by the next `new_len` bytes of `data`.
    /// `old_len == 0` is an insertion, `new_len == 0` a deletion. Ranges are
    /// strictly ascending and non-overlapping. The third field is why this tuple is
    /// not the plan's `(from, len)`: without separate base and data lengths a
    /// residual cannot express an insertion or a deletion, and the plan's own
    /// `ResidualKind` list includes both.
    RangeReplace {
        ranges: Vec<(u32, u32, u32)>,
        data: Vec<u8>,
    },
    /// `runs[i] = (start, len, byte)`: overwrite `base[start..start+len]` with
    /// `byte`. Same output length as the base.
    RunPatch { runs: Vec<(u32, u32, u8)> },
    /// Same-length substitution whose mismatch positions are addressed by the
    /// colex rank of the `k`-subset of the `n` positions, `k = values.len()`.
    RankedMismatch { mask_rank: u64, values: Vec<u8> },
    /// A copy/insert script.
    EditScript { ops: Vec<EditOp> },
}

impl Residual {
    /// The family this residual belongs to.
    pub fn kind(&self) -> ResidualKind {
        match self {
            Residual::None => ResidualKind::None,
            Residual::SparseSubstitute { .. } => ResidualKind::SparseSubstitute,
            Residual::RangeReplace { .. } => ResidualKind::RangeReplace,
            Residual::RunPatch { .. } => ResidualKind::RunPatch,
            Residual::RankedMismatch { .. } => ResidualKind::RankedMismatch,
            Residual::EditScript { .. } => ResidualKind::EditScript,
        }
    }
}

/// Common prefix / suffix lengths of two slices, non-overlapping.
fn common_ends(base: &[u8], target: &[u8]) -> (usize, usize) {
    let max = base.len().min(target.len());
    let mut prefix = 0;
    while prefix < max && base[prefix] == target[prefix] {
        prefix += 1;
    }
    let mut suffix = 0;
    while suffix < max - prefix
        && base[base.len() - 1 - suffix] == target[target.len() - 1 - suffix]
    {
        suffix += 1;
    }
    (prefix, suffix)
}

/// Apply `r` to `base`, returning the target, or `None` if `r` is malformed or
/// does not fit `base`. Integer-only; never panics.
pub fn apply(base: &[u8], r: &Residual) -> Option<Vec<u8>> {
    match r {
        Residual::None => Some(base.to_vec()),
        Residual::SparseSubstitute { positions, values } => {
            if positions.len() != values.len() {
                return None;
            }
            let mut out = base.to_vec();
            let mut prev: Option<u32> = None;
            for (i, &p) in positions.iter().enumerate() {
                if p as usize >= base.len() {
                    return None;
                }
                if let Some(q) = prev {
                    if p <= q {
                        return None;
                    }
                }
                prev = Some(p);
                out[p as usize] = values[i];
            }
            Some(out)
        }
        Residual::RangeReplace { ranges, data } => {
            let mut out = Vec::with_capacity(base.len());
            let mut cursor = 0usize;
            let mut data_at = 0usize;
            let mut prev_end = 0usize;
            for (i, &(from, old_len, new_len)) in ranges.iter().enumerate() {
                let from = from as usize;
                let old_len = old_len as usize;
                let new_len = new_len as usize;
                let end = from.checked_add(old_len)?;
                if end > base.len() {
                    return None;
                }
                if i > 0 && from < prev_end {
                    return None;
                }
                out.extend_from_slice(&base[cursor..from]);
                let data_end = data_at.checked_add(new_len)?;
                if data_end > data.len() {
                    return None;
                }
                out.extend_from_slice(&data[data_at..data_end]);
                data_at = data_end;
                cursor = end;
                prev_end = end;
            }
            out.extend_from_slice(&base[cursor..]);
            if data_at != data.len() {
                return None;
            }
            Some(out)
        }
        Residual::RunPatch { runs } => {
            let mut out = base.to_vec();
            let mut prev_end = 0usize;
            for (i, &(start, len, byte)) in runs.iter().enumerate() {
                let start = start as usize;
                let len = len as usize;
                let end = start.checked_add(len)?;
                if end > base.len() {
                    return None;
                }
                if i > 0 && start < prev_end {
                    return None;
                }
                for slot in &mut out[start..end] {
                    *slot = byte;
                }
                prev_end = end;
            }
            Some(out)
        }
        Residual::RankedMismatch { mask_rank, values } => {
            if base.len() > u32::MAX as usize {
                return None;
            }
            let n = base.len() as u32;
            let k = values.len() as u32;
            let positions = unrank_subset(n, k, *mask_rank)?;
            let mut out = base.to_vec();
            for (i, &p) in positions.iter().enumerate() {
                out[p as usize] = values[i];
            }
            Some(out)
        }
        Residual::EditScript { ops } => {
            let mut out = Vec::new();
            for op in ops {
                match op {
                    EditOp::Copy { from, len } => {
                        let from = *from as usize;
                        let len = *len as usize;
                        let end = from.checked_add(len)?;
                        if end > base.len() {
                            return None;
                        }
                        out.extend_from_slice(&base[from..end]);
                    }
                    EditOp::Insert(data) => out.extend_from_slice(data),
                }
            }
            Some(out)
        }
    }
}

/// Derive a residual of `kind` that maps `base` to `target`, or `None` if no
/// residual of that kind can (or if the space is too large to address in `u64`).
///
/// The derivation is canonical: the same `(base, target, kind)` always yields the
/// same bytes, so a round-trip test is deterministic and an archive is
/// reproducible.
pub fn derive(base: &[u8], target: &[u8], kind: ResidualKind) -> Option<Residual> {
    match kind {
        ResidualKind::None => {
            if base == target {
                Some(Residual::None)
            } else {
                None
            }
        }
        ResidualKind::SparseSubstitute => {
            if base.len() != target.len() {
                return None;
            }
            let mut positions = Vec::new();
            let mut values = Vec::new();
            for (i, (&b, &t)) in base.iter().zip(target.iter()).enumerate() {
                if b != t {
                    positions.push(i as u32);
                    values.push(t);
                }
            }
            Some(Residual::SparseSubstitute { positions, values })
        }
        ResidualKind::RangeReplace => {
            if base.len() > u32::MAX as usize || target.len() > u32::MAX as usize {
                return None;
            }
            let (prefix, suffix) = common_ends(base, target);
            let base_mid = base.len() - prefix - suffix;
            let tgt_mid = target.len() - prefix - suffix;
            let (ranges, data) = if base_mid == 0 && tgt_mid == 0 {
                (Vec::new(), Vec::new())
            } else {
                (
                    vec![(prefix as u32, base_mid as u32, tgt_mid as u32)],
                    target[prefix..target.len() - suffix].to_vec(),
                )
            };
            Some(Residual::RangeReplace { ranges, data })
        }
        ResidualKind::RunPatch => {
            if base.len() != target.len() {
                return None;
            }
            let mut runs: Vec<(u32, u32, u8)> = Vec::new();
            for (i, (&b, &t)) in base.iter().zip(target.iter()).enumerate() {
                if b == t {
                    continue;
                }
                match runs.last_mut() {
                    // A run continues only while the *replacement byte* is constant
                    // and the positions are contiguous. Contiguity matters: an equal
                    // byte between two mismatches must not be swallowed by the run.
                    Some((start, len, byte))
                        if *byte == t && (*start as usize + *len as usize) == i =>
                    {
                        *len += 1
                    }
                    _ => runs.push((i as u32, 1, t)),
                }
            }
            Some(Residual::RunPatch { runs })
        }
        ResidualKind::RankedMismatch => {
            if base.len() != target.len() || base.len() > u32::MAX as usize {
                return None;
            }
            let mut positions = Vec::new();
            let mut values = Vec::new();
            for (i, (&b, &t)) in base.iter().zip(target.iter()).enumerate() {
                if b != t {
                    positions.push(i as u32);
                    values.push(t);
                }
            }
            let mask_rank = rank_subset(base.len() as u32, values.len() as u32, &positions)?;
            Some(Residual::RankedMismatch { mask_rank, values })
        }
        ResidualKind::EditScript => {
            if base.len() > u32::MAX as usize || target.len() > u32::MAX as usize {
                return None;
            }
            let (prefix, suffix) = common_ends(base, target);
            let mut ops = Vec::new();
            if prefix > 0 {
                ops.push(EditOp::Copy {
                    from: 0,
                    len: prefix as u32,
                });
            }
            let mid = &target[prefix..target.len() - suffix];
            if !mid.is_empty() {
                ops.push(EditOp::Insert(mid.to_vec()));
            }
            if suffix > 0 {
                ops.push(EditOp::Copy {
                    from: (base.len() - suffix) as u32,
                    len: suffix as u32,
                });
            }
            Some(Residual::EditScript { ops })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A deterministic xorshift so the property tests are reproducible and do not
    /// depend on a crate, a clock or a hash seed.
    struct Rng(u64);
    impl Rng {
        fn new(seed: u64) -> Self {
            Rng(seed | 1)
        }
        fn next_u64(&mut self) -> u64 {
            let mut x = self.0;
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            self.0 = x;
            x
        }
        fn below(&mut self, n: u32) -> u32 {
            if n == 0 {
                0
            } else {
                (self.next_u64() % n as u64) as u32
            }
        }
    }

    /// Small-alphabet random bytes, so matches and mismatches both occur often.
    fn random_bytes(rng: &mut Rng, max_len: usize) -> Vec<u8> {
        let len = rng.below((max_len + 1) as u32) as usize;
        (0..len).map(|_| rng.below(4) as u8).collect()
    }

    const KINDS: [ResidualKind; 6] = [
        ResidualKind::None,
        ResidualKind::SparseSubstitute,
        ResidualKind::RangeReplace,
        ResidualKind::RunPatch,
        ResidualKind::RankedMismatch,
        ResidualKind::EditScript,
    ];

    #[test]
    fn derive_then_apply_reproduces_the_target() {
        // The central property: whenever a residual of the requested kind exists,
        // applying it to the base recovers the target exactly.
        let mut rng = Rng::new(0xC0FFEE);
        for _ in 0..2000 {
            let base = random_bytes(&mut rng, 12);
            let target = random_bytes(&mut rng, 12);
            for kind in KINDS {
                if let Some(r) = derive(&base, &target, kind) {
                    assert_eq!(
                        apply(&base, &r).unwrap(),
                        target,
                        "kind {kind:?} base {base:?} target {target:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn equal_length_shapes_always_derive() {
        // These three shapes preserve length, so for equal-length inputs a residual
        // of each must exist and round-trip.
        let mut rng = Rng::new(0x1234_5678);
        for _ in 0..2000 {
            let n = rng.below(13) as usize;
            let base: Vec<u8> = (0..n).map(|_| rng.below(4) as u8).collect();
            let target: Vec<u8> = (0..n).map(|_| rng.below(4) as u8).collect();
            for kind in [
                ResidualKind::SparseSubstitute,
                ResidualKind::RunPatch,
                ResidualKind::RankedMismatch,
            ] {
                let r = derive(&base, &target, kind)
                    .unwrap_or_else(|| panic!("kind {kind:?} should derive for equal lengths"));
                assert_eq!(apply(&base, &r).unwrap(), target);
            }
        }
    }

    #[test]
    fn general_shapes_always_derive_for_any_lengths() {
        let mut rng = Rng::new(0xDEAD_BEEF);
        for _ in 0..2000 {
            let base = random_bytes(&mut rng, 16);
            let target = random_bytes(&mut rng, 16);
            for kind in [ResidualKind::RangeReplace, ResidualKind::EditScript] {
                let r =
                    derive(&base, &target, kind).expect("general shape must represent any change");
                assert_eq!(apply(&base, &r).unwrap(), target);
            }
        }
    }

    #[test]
    fn none_derives_only_for_identical_bytes() {
        assert_eq!(
            derive(b"abc", b"abc", ResidualKind::None),
            Some(Residual::None)
        );
        assert_eq!(derive(b"abc", b"abd", ResidualKind::None), None);
        assert_eq!(derive(b"abc", b"ab", ResidualKind::None), None);
    }

    #[test]
    fn sparse_substitute_names_only_the_changed_positions() {
        let r = derive(b"abcdef", b"aXcYeZ", ResidualKind::SparseSubstitute).unwrap();
        assert_eq!(
            r,
            Residual::SparseSubstitute {
                positions: vec![1, 3, 5],
                values: vec![b'X', b'Y', b'Z'],
            }
        );
        assert_eq!(apply(b"abcdef", &r).unwrap(), b"aXcYeZ");
    }

    #[test]
    fn range_replace_handles_insertions_and_deletions() {
        let grow = derive(b"abc", b"aXYZc", ResidualKind::RangeReplace).unwrap();
        assert_eq!(apply(b"abc", &grow).unwrap(), b"aXYZc");
        let shrink = derive(b"aXYZc", b"abc", ResidualKind::RangeReplace).unwrap();
        assert_eq!(apply(b"aXYZc", &shrink).unwrap(), b"abc");
    }

    #[test]
    fn run_patch_overwrites_constant_runs() {
        let r = derive(b"aaaabbbbcc", b"aZZZbbbbcc", ResidualKind::RunPatch).unwrap();
        assert_eq!(apply(b"aaaabbbbcc", &r).unwrap(), b"aZZZbbbbcc");
        // A run stops where the replacement byte changes, so different bytes are
        // separate runs rather than one run with a varying byte.
        let r = derive(b"aaaa", b"XYZW", ResidualKind::RunPatch).unwrap();
        assert_eq!(
            r,
            Residual::RunPatch {
                runs: vec![(0, 1, b'X'), (1, 1, b'Y'), (2, 1, b'Z'), (3, 1, b'W')]
            }
        );
    }

    #[test]
    fn ranked_mismatch_agrees_with_sparse_substitute() {
        let base = b"mnopqrst";
        let target = b"mnoXqrsY";
        let sparse = derive(base, target, ResidualKind::SparseSubstitute).unwrap();
        let ranked = derive(base, target, ResidualKind::RankedMismatch).unwrap();
        match (&sparse, &ranked) {
            (
                Residual::SparseSubstitute { positions, .. },
                Residual::RankedMismatch { mask_rank, .. },
            ) => {
                assert_eq!(
                    rank_subset(base.len() as u32, positions.len() as u32, positions),
                    Some(*mask_rank)
                );
            }
            _ => panic!("unexpected residual shapes"),
        }
        assert_eq!(apply(base, &ranked).unwrap(), target);
    }

    #[test]
    fn edit_script_copies_the_shared_ends_and_inserts_the_middle() {
        let r = derive(
            b"hello world",
            b"hello, brave world",
            ResidualKind::EditScript,
        )
        .unwrap();
        assert_eq!(apply(b"hello world", &r).unwrap(), b"hello, brave world");
        assert_eq!(
            r,
            Residual::EditScript {
                ops: vec![
                    EditOp::Copy { from: 0, len: 5 },
                    EditOp::Insert(b", brave".to_vec()),
                    EditOp::Copy { from: 5, len: 6 },
                ]
            }
        );
    }

    #[test]
    fn derivation_is_deterministic() {
        let mut rng = Rng::new(7);
        for _ in 0..500 {
            let base = random_bytes(&mut rng, 10);
            let target = random_bytes(&mut rng, 10);
            for kind in KINDS {
                assert_eq!(derive(&base, &target, kind), derive(&base, &target, kind));
            }
        }
    }

    #[test]
    fn apply_rejects_malformed_residuals() {
        // Every rejection is a `None`, never a panic and never a partly-applied
        // buffer.
        let base = b"abc";
        let cases = [
            Residual::SparseSubstitute {
                positions: vec![0],
                values: vec![],
            },
            Residual::SparseSubstitute {
                positions: vec![10],
                values: vec![b'X'],
            },
            Residual::SparseSubstitute {
                positions: vec![2, 1],
                values: vec![b'X', b'Y'],
            },
            Residual::RangeReplace {
                ranges: vec![(0, 2, 2), (1, 1, 1)],
                data: vec![1, 2, 3],
            },
            Residual::RangeReplace {
                ranges: vec![(0, 2, 1)],
                data: vec![],
            },
            Residual::RangeReplace {
                ranges: vec![(0, 9, 9)],
                data: vec![1; 9],
            },
            Residual::RunPatch {
                runs: vec![(1, 9, 0)],
            },
            Residual::RankedMismatch {
                mask_rank: 999,
                values: vec![b'X'],
            },
            Residual::EditScript {
                ops: vec![EditOp::Copy { from: 1, len: 9 }],
            },
        ];
        for r in cases {
            assert_eq!(apply(base, &r), None, "should reject {r:?}");
        }
    }

    #[test]
    fn empty_inputs_are_handled() {
        assert_eq!(apply(b"", &Residual::None).unwrap(), b"");
        let r = derive(b"", b"", ResidualKind::EditScript).unwrap();
        assert_eq!(apply(b"", &r).unwrap(), b"");
        let r = derive(b"", b"xy", ResidualKind::RangeReplace).unwrap();
        assert_eq!(apply(b"", &r).unwrap(), b"xy");
        let r = derive(b"xy", b"", ResidualKind::RangeReplace).unwrap();
        assert_eq!(apply(b"xy", &r).unwrap(), b"");
    }
}
