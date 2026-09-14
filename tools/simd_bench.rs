//! Research-plane microbenchmark: is AVX2 actually faster than scalar for the
//! two hot patterns in the Zentropy predictor?
//!
//!   A) random 32-bit table lookup + update — the per-expert context path.
//!      This is a read-modify-write into a hashed table far larger than cache.
//!   B) contiguous dot product — the mixer.
//!
//! Not part of the scored artifact; lives in gitignored `research/`.
//!
//! Build: rustc -O -C target-cpu=native research/simd_bench.rs -o /tmp/simd_bench

use std::time::Instant;

#[cfg(target_arch = "x86_64")]
use std::arch::x86_64::*;

fn splitmix64(mut x: u64) -> u64 {
    x = x.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = x;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// A) scalar: 8 independent random read-modify-writes.
#[inline(never)]
fn lookup_scalar(table: &mut [u32], idx: &[usize; 8]) -> u32 {
    let mut acc = 0u32;
    for &i in idx.iter() {
        let v = table[i];
        acc = acc.wrapping_add(v);
        table[i] = v.wrapping_add(1);
    }
    acc
}

/// A') scalar + software prefetch of the next round's lines.
#[inline(never)]
fn lookup_scalar_prefetch(table: &mut [u32], idx: &[usize; 8], next: &[usize; 8]) -> u32 {
    #[cfg(target_arch = "x86_64")]
    for &i in next.iter() {
        unsafe {
            let p = table.as_ptr().add(i) as *const i8;
            _mm_prefetch(p, _MM_HINT_T0);
        }
    }
    lookup_scalar(table, idx)
}

/// A) AVX2: gather, then still update with 8 scalar stores (AVX2 has no scatter).
#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
unsafe fn lookup_avx2(table: &mut [u32], idx: &[usize; 8]) -> u32 {
    let vi = _mm256_setr_epi32(
        idx[0] as i32,
        idx[1] as i32,
        idx[2] as i32,
        idx[3] as i32,
        idx[4] as i32,
        idx[5] as i32,
        idx[6] as i32,
        idx[7] as i32,
    );
    let base = table.as_ptr() as *const i32;
    let g = _mm256_i32gather_epi32::<4>(base, vi);
    let mut out = [0i32; 8];
    _mm256_storeu_si256(out.as_mut_ptr() as *mut __m256i, g);
    let mut acc = 0u32;
    for k in 0..8 {
        acc = acc.wrapping_add(out[k] as u32);
        table[idx[k]] = (out[k] as u32).wrapping_add(1);
    }
    acc
}

/// B) scalar dot product of i16 pairs accumulated into i32.
#[inline(never)]
fn mix_scalar(w: &[i16], x: &[i16]) -> i32 {
    let mut acc = 0i32;
    for i in 0..w.len() {
        acc = acc.wrapping_add((w[i] as i32).wrapping_mul(x[i] as i32));
    }
    acc
}

/// B) AVX2 dot product: 16 pairs per instruction via vpmaddwd.
#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
unsafe fn mix_avx2(w: &[i16], x: &[i16]) -> i32 {
    let mut acc = _mm256_setzero_si256();
    let n = w.len() / 16;
    for i in 0..n {
        let wv = _mm256_loadu_si256(w.as_ptr().add(i * 16) as *const __m256i);
        let xv = _mm256_loadu_si256(x.as_ptr().add(i * 16) as *const __m256i);
        acc = _mm256_add_epi32(acc, _mm256_madd_epi16(wv, xv));
    }
    let mut lanes = [0i32; 8];
    _mm256_storeu_si256(lanes.as_mut_ptr() as *mut __m256i, acc);
    let mut s = 0i32;
    for v in lanes {
        s = s.wrapping_add(v);
    }
    let tail = n * 16;
    for i in tail..w.len() {
        s = s.wrapping_add((w[i] as i32).wrapping_mul(x[i] as i32));
    }
    s
}

// ---- i32 (16.16) variant, matching the real mixer's integer types ----------

/// Scalar 16.16 mixer dot: (w*x) >> 16, accumulated in i64.
#[inline(never)]
fn mix32_scalar(w: &[i32], x: &[i32]) -> i64 {
    let mut acc = 0i64;
    for i in 0..w.len() {
        acc = acc.wrapping_add(((w[i] as i64) * (x[i] as i64)) >> 16);
    }
    acc
}

/// AVX2 16.16 mixer dot: vpmulld (4 i32 lanes) with 64-bit accumulation.
#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
unsafe fn mix32_avx2(w: &[i32], x: &[i32]) -> i64 {
    let mut lo = _mm256_setzero_si256(); // 4 x i64
    let mut hi = _mm256_setzero_si256();
    let zero = _mm256_setzero_si256();
    let n = w.len() / 8;
    for i in 0..n {
        let wv = _mm256_loadu_si256(w.as_ptr().add(i * 8) as *const __m256i);
        let xv = _mm256_loadu_si256(x.as_ptr().add(i * 8) as *const __m256i);
        let p = _mm256_mullo_epi32(wv, xv); // low 32 bits of each product
                                            // Widen the even lanes and the odd lanes to 64-bit and accumulate.
        let even = _mm256_srai_epi32::<16>(p);
        let odd = _mm256_srai_epi32::<16>(_mm256_srli_epi64::<32>(p));
        let ev = _mm256_srai_epi32::<31>(even);
        let od = _mm256_srai_epi32::<31>(odd);
        let evq = _mm256_unpacklo_epi32(even, ev);
        let odq = _mm256_unpacklo_epi32(odd, od);
        lo = _mm256_add_epi64(lo, evq);
        hi = _mm256_add_epi64(hi, odq);
        let _ = zero;
    }
    let s = _mm256_add_epi64(lo, hi);
    let mut lanes = [0i64; 4];
    _mm256_storeu_si256(lanes.as_mut_ptr() as *mut __m256i, s);
    let mut acc = lanes.iter().sum::<i64>();
    for i in n * 8..w.len() {
        acc = acc.wrapping_add(((w[i] as i64) * (x[i] as i64)) >> 16);
    }
    acc
}

fn main() {
    // ---- A: random table lookup+update -------------------------------------
    const TABLE_BITS: usize = 24; // 64 MiB of u32: far beyond L2, mostly beyond L3
    let table_len = 1usize << TABLE_BITS;
    let mut table = vec![0u32; table_len];
    for (i, v) in table.iter_mut().enumerate() {
        *v = (i as u32).wrapping_mul(2654435761);
    }

    let n_rounds = 2_000_000usize;
    let mut idxs = vec![[0usize; 8]; n_rounds];
    let mut s = 0x1234_5678u64;
    for r in idxs.iter_mut() {
        for k in 0..8 {
            s = splitmix64(s);
            r[k] = (s as usize) & (table_len - 1);
        }
    }

    let mut sink = 0u32;

    let t = Instant::now();
    for r in idxs.iter() {
        sink = sink.wrapping_add(lookup_scalar(&mut table, r));
    }
    let d_scalar = t.elapsed();

    // Drain the table's cache footprint between runs by touching it stride-wise.
    let t = Instant::now();
    for r in idxs.iter() {
        sink = sink.wrapping_add(lookup_scalar_prefetch(&mut table, r, r));
    }
    let d_prefetch = t.elapsed();

    #[cfg(target_arch = "x86_64")]
    let d_avx2 = {
        let t = Instant::now();
        for r in idxs.iter() {
            sink = sink.wrapping_add(unsafe { lookup_avx2(&mut table, r) });
        }
        t.elapsed()
    };
    #[cfg(not(target_arch = "x86_64"))]
    let d_avx2 = d_scalar;

    let per = |d: std::time::Duration| d.as_secs_f64() * 1e9 / (n_rounds as f64 * 8.0);
    println!(
        "A) random table lookup+update (8 per round, {n_rounds} rounds, {} MiB table)",
        table_len * 4 / (1024 * 1024)
    );
    println!(
        "   scalar          {:>7.2} ns/lookup   {:>8.3} s",
        per(d_scalar),
        d_scalar.as_secs_f64()
    );
    println!(
        "   scalar+prefetch {:>7.2} ns/lookup   {:>8.3} s",
        per(d_prefetch),
        d_prefetch.as_secs_f64()
    );
    println!(
        "   avx2 gather     {:>7.2} ns/lookup   {:>8.3} s",
        per(d_avx2),
        d_avx2.as_secs_f64()
    );
    println!(
        "   ratio avx2/scalar = {:.3}  (<1 means AVX2 wins)",
        d_avx2.as_secs_f64() / d_scalar.as_secs_f64()
    );
    println!(
        "   ratio prefetch/scalar = {:.3}",
        d_prefetch.as_secs_f64() / d_scalar.as_secs_f64()
    );

    // ---- B: mixer dot product ---------------------------------------------
    // The operands must change each iteration or LLVM hoists the whole dot
    // product out of the loop as loop-invariant (which is what the first version
    // of this benchmark did). Rotating one lane does that and keeps the operands
    // in L1, which is the real mixer's situation.
    let n_inputs = 64usize;
    let w: Vec<i16> = (0..n_inputs)
        .map(|i| ((i as i16) * 31 % 4096) - 2048)
        .collect();
    let mut x: Vec<i16> = (0..n_inputs)
        .map(|i| ((i as i16) * 17 % 2048) - 1024)
        .collect();
    let n_mix = 50_000_000usize;

    let t = Instant::now();
    for i in 0..n_mix {
        x[i % n_inputs] = ((i as i16) & 2047) - 1024;
        sink = sink.wrapping_add(mix_scalar(&w, &x) as u32);
    }
    let m_scalar = t.elapsed();

    #[cfg(target_arch = "x86_64")]
    let m_avx2 = {
        let t = Instant::now();
        for i in 0..n_mix {
            x[i % n_inputs] = ((i as i16) & 2047) - 1024;
            sink = sink.wrapping_add(unsafe { mix_avx2(&w, &x) } as u32);
        }
        t.elapsed()
    };
    #[cfg(not(target_arch = "x86_64"))]
    let m_avx2 = m_scalar;

    println!();
    println!("B) mixer dot product, i16 ({n_inputs} inputs, {n_mix} mixes)");
    println!(
        "   scalar {:>7.3} ns/mix   {:>8.3} s",
        m_scalar.as_secs_f64() * 1e9 / n_mix as f64,
        m_scalar.as_secs_f64()
    );
    println!(
        "   avx2   {:>7.3} ns/mix   {:>8.3} s",
        m_avx2.as_secs_f64() * 1e9 / n_mix as f64,
        m_avx2.as_secs_f64()
    );
    println!(
        "   speedup avx2 = {:.2}x",
        m_scalar.as_secs_f64() / m_avx2.as_secs_f64()
    );

    // ---- B': the real mixer's integer type (16.16 fixed point, i32) --------
    let w32: Vec<i32> = (0..n_inputs)
        .map(|i| (((i as i32) * 31 % 4096) - 2048) << 8)
        .collect();
    let mut x32: Vec<i32> = (0..n_inputs)
        .map(|i| ((i as i32) * 17 % 2048) - 1024)
        .collect();

    let t = Instant::now();
    for i in 0..n_mix {
        x32[i % n_inputs] = ((i as i32) & 2047) - 1024;
        sink = sink.wrapping_add(mix32_scalar(&w32, &x32) as u32);
    }
    let m32_scalar = t.elapsed();

    #[cfg(target_arch = "x86_64")]
    let m32_avx2 = {
        let t = Instant::now();
        for i in 0..n_mix {
            x32[i % n_inputs] = ((i as i32) & 2047) - 1024;
            sink = sink.wrapping_add(unsafe { mix32_avx2(&w32, &x32) } as u32);
        }
        t.elapsed()
    };
    #[cfg(not(target_arch = "x86_64"))]
    let m32_avx2 = m32_scalar;

    println!();
    println!("B') mixer dot product, i32 16.16 (the real mixer's types)");
    println!(
        "   scalar {:>7.3} ns/mix   {:>8.3} s",
        m32_scalar.as_secs_f64() * 1e9 / n_mix as f64,
        m32_scalar.as_secs_f64()
    );
    println!(
        "   avx2   {:>7.3} ns/mix   {:>8.3} s",
        m32_avx2.as_secs_f64() * 1e9 / n_mix as f64,
        m32_avx2.as_secs_f64()
    );
    println!(
        "   speedup avx2 = {:.2}x",
        m32_scalar.as_secs_f64() / m32_avx2.as_secs_f64()
    );

    println!();
    println!("(sink {sink})");
}
