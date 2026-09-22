//! Rank and unrank primitives for state coding (plan §4.3).
//!
//! A *rank* is an integer coordinate for a combinatorial object: a `k`-subset of
//! `{0..n}`, a permutation, or a mixed-radix digit vector. The point of coding
//! state as a rank rather than as a raw list is that the rank is a single small
//! integer which the coder can then spend bytes on, and the object is recovered
//! exactly by `unrank`. Every pair here is an exact inverse.
//!
//! **No panics, no silent wraparound.** A rank that does not fit in `u64`, or an
//! input that is not a legal coordinate for its space, returns `None` rather than
//! panicking or aliasing a different object. That is a deliberate departure from
//! the plan's `rank_subset(..) -> u64` sketch: a rank function that must not panic
//! cannot also return a bare integer on invalid input, and a sentinel like `0`
//! would silently turn a forged coordinate into the empty set.

/// `C(n, k)` computed exactly, or `None` if it exceeds `u128`.
///
/// `u128` is the working width because it is wide enough for every rank this
/// module can return (`u64`) with headroom to detect overflow; a rank that does
/// not fit `u64` is unreserved, not clamped.
fn binomial(n: u64, k: u64) -> Option<u128> {
    if k > n {
        return Some(0);
    }
    let k = k.min(n - k);
    let mut r: u128 = 1;
    for i in 0..k {
        r = r.checked_mul((n - i) as u128)?;
        r /= (i + 1) as u128;
    }
    Some(r)
}

/// `k!` for `k <= 20`, or `None` beyond (where it would not fit a `u64` anyway).
fn factorial(k: u32) -> Option<u128> {
    let mut r: u128 = 1;
    for i in 2..=k as u64 {
        r = r.checked_mul(i as u128)?;
    }
    Some(r)
}

/// Colexicographic rank of a `k`-subset of `{0, 1, ..., n-1}`.
///
/// `set` must be strictly ascending, in range, and of length `k`; anything else is
/// `None`. The result is `Σ_i C(set[i], i+1)`, the standard combinatorial number
/// system, which is a bijection onto `0..C(n, k)`.
pub fn rank_subset(n: u32, k: u32, set: &[u32]) -> Option<u64> {
    if set.len() != k as usize || k > n {
        return None;
    }
    let mut r: u128 = 0;
    let mut prev: Option<u32> = None;
    for (i, &c) in set.iter().enumerate() {
        if c >= n {
            return None;
        }
        if let Some(p) = prev {
            if c <= p {
                return None;
            }
        }
        prev = Some(c);
        r = r.checked_add(binomial(c as u64, (i + 1) as u64)?)?;
    }
    if r > u64::MAX as u128 {
        return None;
    }
    Some(r as u64)
}

/// Inverse of [`rank_subset`]: the `k`-subset of `{0..n}` with colex rank `r`.
///
/// Returns `None` if `k > n`, if `r` is outside `0..C(n, k)`, or if the space is
/// too large to rank in `u64`. Uses binary search per position, so a huge `n` with
/// a small `k` cannot make this hang.
pub fn unrank_subset(n: u32, k: u32, r: u64) -> Option<Vec<u32>> {
    if k > n {
        return None;
    }
    let total = binomial(n as u64, k as u64)?;
    if (r as u128) >= total {
        return None;
    }
    let mut out = vec![0u32; k as usize];
    let mut rem = r as u128;
    let mut hi = n as u64; // exclusive upper bound for the next element
    for i in (1..=k).rev() {
        let i = i as u64;
        let lo_bound = i - 1;
        // Largest c in [i-1, hi) with C(c, i) <= rem. C is monotone in c, so the
        // first c with C(c, i) > rem is found by binary search.
        let (mut lo, mut h) = (lo_bound, hi);
        while lo < h {
            let mid = lo + (h - lo) / 2;
            if binomial(mid, i)? <= rem {
                lo = mid + 1;
            } else {
                h = mid;
            }
        }
        let c = lo.checked_sub(1)?;
        if c < lo_bound {
            return None;
        }
        rem -= binomial(c, i)?;
        out[(i - 1) as usize] = c as u32;
        hi = c;
    }
    if rem != 0 {
        return None;
    }
    Some(out)
}

/// Lehmer (factoradic) rank of a permutation of `{0..n-1}`.
///
/// Returns `None` if `perm` is not a permutation, or if `n > 20` (where `n!`
/// exceeds `u64`, so the coordinate space does not fit and no rank could be
/// unique).
pub fn rank_permutation(perm: &[u32]) -> Option<u64> {
    let n = perm.len() as u32;
    if n > 20 {
        return None;
    }
    let mut seen = vec![false; n as usize];
    for &x in perm {
        if x >= n || seen[x as usize] {
            return None;
        }
        seen[x as usize] = true;
    }
    let mut avail: Vec<u32> = (0..n).collect();
    let mut rank: u128 = 0;
    for (i, &x) in perm.iter().enumerate() {
        let pos = avail.iter().position(|&y| y == x)?;
        let f = factorial(n - 1 - i as u32)?;
        rank = rank.checked_add(pos as u128 * f)?;
        avail.remove(pos);
    }
    if rank > u64::MAX as u128 {
        return None;
    }
    Some(rank as u64)
}

/// Inverse of [`rank_permutation`]: the permutation with Lehmer rank `r`.
///
/// Returns `None` if `n > 20` or `r >= n!`.
pub fn unrank_permutation(n: u32, r: u64) -> Option<Vec<u32>> {
    if n > 20 {
        return None;
    }
    let total = factorial(n)?;
    if (r as u128) >= total {
        return None;
    }
    let mut avail: Vec<u32> = (0..n).collect();
    let mut out = Vec::with_capacity(n as usize);
    let mut rem = r as u128;
    for i in 0..n {
        let f = factorial(n - 1 - i)?;
        let idx = (rem / f) as usize;
        rem %= f;
        out.push(avail.remove(idx));
    }
    Some(out)
}

// --- mixed radix -----------------------------------------------------------
//
// The rank can exceed `u64` (the product of the radices is the state-space size),
// so it is a little-endian vector of base-2^64 limbs, canonical: no trailing zero
// limbs, and zero is `[0]`.

fn bi_is_zero(a: &[u64]) -> bool {
    a.iter().all(|&x| x == 0)
}

fn bi_canonical(mut a: Vec<u64>) -> Vec<u64> {
    while a.len() > 1 && a.last() == Some(&0) {
        a.pop();
    }
    if a.is_empty() {
        a.push(0);
    }
    a
}

/// `a *= m`, in place. `m` is a radix or digit, so a `u32`.
fn bi_mul_small(a: &mut Vec<u64>, m: u32) {
    if m == 0 {
        a.clear();
        a.push(0);
        return;
    }
    let mut carry: u128 = 0;
    for limb in a.iter_mut() {
        let cur = (*limb as u128) * (m as u128) + carry;
        *limb = cur as u64;
        carry = cur >> 64;
    }
    while carry != 0 {
        a.push(carry as u64);
        carry >>= 64;
    }
}

/// `a += v`, in place.
fn bi_add(a: &mut Vec<u64>, b: &[u64]) {
    let n = a.len().max(b.len());
    a.resize(n, 0);
    let mut carry: u128 = 0;
    for i in 0..n {
        let bv = if i < b.len() { b[i] as u128 } else { 0 };
        let cur = (a[i] as u128) + bv + carry;
        a[i] = cur as u64;
        carry = cur >> 64;
    }
    if carry != 0 {
        a.push(carry as u64);
    }
}

/// `a /= d`, returning the remainder. `d` must be non-zero and a `u32`.
fn bi_divmod_small(a: &mut Vec<u64>, d: u32) -> u32 {
    let mut rem: u128 = 0;
    for limb in a.iter_mut().rev() {
        let cur = (rem << 64) | (*limb as u128);
        *limb = (cur / d as u128) as u64;
        rem = cur % d as u128;
    }
    rem as u32
}

/// Mixed-radix rank of `digits`: `Σ_i digits[i] * Π_{j<i} radices[j]`.
///
/// `digits[i]` must be `< radices[i]`; anything else is `None` (a digit outside
/// its radix is not a coordinate). The result is a little-endian limb vector
/// because the value can exceed `u64`. Inverts [`unrank_mixed_radix`].
pub fn rank_mixed_radix(radices: &[u32], digits: &[u32]) -> Option<Vec<u64>> {
    if radices.len() != digits.len() {
        return None;
    }
    let mut rank: Vec<u64> = vec![0];
    let mut place: Vec<u64> = vec![1];
    for i in 0..radices.len() {
        if digits[i] >= radices[i] {
            return None;
        }
        if digits[i] != 0 {
            let mut term = place.clone();
            bi_mul_small(&mut term, digits[i]);
            bi_add(&mut rank, &term);
        }
        bi_mul_small(&mut place, radices[i]);
    }
    Some(bi_canonical(rank))
}

/// Inverse of [`rank_mixed_radix`]: recover the digits from `rank`.
///
/// Returns `None` if `rank` is outside `0..Π radices` (no such digit vector), or
/// if any radix is zero (a zero-width digit is not a coordinate).
pub fn unrank_mixed_radix(radices: &[u32], rank: &[u64]) -> Option<Vec<u32>> {
    let mut quot = bi_canonical(rank.to_vec());
    let mut digits = Vec::with_capacity(radices.len());
    for &r in radices {
        if r == 0 {
            return None;
        }
        digits.push(bi_divmod_small(&mut quot, r));
    }
    if !bi_is_zero(&quot) {
        return None;
    }
    Some(digits)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_subset_round_trips_through_its_colex_rank() {
        for n in 0..=12u32 {
            for k in 0..=n {
                let total = binomial(n as u64, k as u64).unwrap() as u64;
                for r in 0..total {
                    let set = unrank_subset(n, k, r).unwrap();
                    assert_eq!(set.len(), k as usize);
                    assert!(set.windows(2).all(|w| w[0] < w[1]), "unordered {set:?}");
                    assert!(set.iter().all(|&x| x < n), "out of range {set:?}");
                    assert_eq!(rank_subset(n, k, &set), Some(r), "n={n} k={k} r={r}");
                }
            }
        }
    }

    #[test]
    fn out_of_range_subset_ranks_are_rejected() {
        assert_eq!(unrank_subset(4, 2, 6), None); // C(4,2) = 6, so 0..6
        assert_eq!(unrank_subset(3, 5, 0), None); // k > n
        assert_eq!(unrank_subset(4, 2, 5).map(|v| v.len()), Some(2));
    }

    #[test]
    fn invalid_subsets_are_rejected_by_rank() {
        assert_eq!(rank_subset(4, 2, &[0, 4]), None); // value >= n
        assert_eq!(rank_subset(4, 2, &[1, 0]), None); // not ascending
        assert_eq!(rank_subset(4, 2, &[1, 1]), None); // duplicate
        assert_eq!(rank_subset(4, 2, &[1]), None); // wrong length
    }

    #[test]
    fn every_permutation_round_trips_through_its_lehmer_rank() {
        for n in 0..=8u32 {
            let total = factorial(n).unwrap() as u64;
            for r in 0..total {
                let perm = unrank_permutation(n, r).unwrap();
                let mut sorted = perm.clone();
                sorted.sort_unstable();
                assert_eq!(sorted, (0..n).collect::<Vec<_>>());
                assert_eq!(rank_permutation(&perm), Some(r), "n={n} r={r}");
            }
        }
    }

    #[test]
    fn invalid_permutations_and_ranks_are_rejected() {
        assert_eq!(rank_permutation(&[0, 0]), None); // duplicate
        assert_eq!(rank_permutation(&[2, 0]), None); // value >= n
        assert_eq!(unrank_permutation(3, 6), None); // 3! = 6, so 0..6
        assert_eq!(unrank_permutation(21, 0), None); // 21! exceeds u64
        assert_eq!(rank_permutation(&vec![0u32; 21]).map(|_| ()), None);
    }

    #[test]
    fn mixed_radix_round_trips_over_the_whole_space() {
        let cases: &[&[u32]] = &[
            &[],
            &[1, 1, 1],
            &[2, 3, 4],
            &[5, 5, 5, 5],
            &[1, 2, 3, 4, 5],
            &[3, 3, 3, 3, 3, 3, 3],
        ];
        for radices in cases {
            let mut product: u64 = 1;
            for &r in *radices {
                product = product.checked_mul(r as u64).unwrap();
            }
            for r in 0..product {
                let digits = unrank_mixed_radix(radices, &[r]).unwrap();
                assert_eq!(digits.len(), radices.len());
                assert!(digits.iter().zip(*radices).all(|(&d, &rad)| d < rad));
                assert_eq!(rank_mixed_radix(radices, &digits), Some(vec![r]), "r={r}");
            }
        }
    }

    #[test]
    fn mixed_radix_rejects_out_of_radix_digits() {
        assert_eq!(rank_mixed_radix(&[3, 3], &[3, 0]), None);
        assert_eq!(rank_mixed_radix(&[3, 3], &[1]), None); // length mismatch
        assert_eq!(unrank_mixed_radix(&[3, 3], &[9]), None); // 9 >= 9
        assert_eq!(unrank_mixed_radix(&[0], &[0]), None); // zero-width digit
    }

    #[test]
    fn a_mixed_radix_rank_can_exceed_one_limb() {
        // Five 16-bit radices span 2^80, beyond u64; the rank must carry a second
        // limb and still invert exactly.
        let radices = [1u32 << 16; 5];
        let digits = [0xFFFFu32; 5];
        let rank = rank_mixed_radix(&radices, &digits).unwrap();
        assert!(rank.len() >= 2, "rank {rank:?} fits one limb");
        assert_eq!(unrank_mixed_radix(&radices, &rank), Some(digits.to_vec()));
        assert_eq!(rank_mixed_radix(&radices, &[0; 5]), Some(vec![0]));
    }
}
