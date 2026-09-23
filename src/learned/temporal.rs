//! Phase 14.34–14.36 — the **temporal** residual corrector.
//!
//! The Phase-8 corrector is *memoryless*: its features are the classical outputs
//! for the current bit only, so it can sharpen a calibrated probability but it
//! cannot see a run. This module adds a second, independent corrector whose
//! features are **causal sequence state** — the history the classical chain
//! already carries but never exposes as a feature vector:
//!
//! * the previous coded bit and the length of the current bit run;
//! * how long the match model's byte predictions have been correct, and a
//!   smoothed correctness level;
//! * the repeat-offset confidence and whether a repeat was active;
//! * the correction *this* corrector emitted for the previous bit (a genuine
//!   recurrent feature);
//! * the previous bit's applied probability;
//! * the previous byte's class and whether it repeated.
//!
//! Every one of these is a pure function of the bytes already coded, so it is
//! derivable on the decoder path; the state machine that maintains them is
//! advanced identically by encoder and decoder.
//!
//! Scored-path properties mirror `crate::learned`: inference is fixed-point
//! integer arithmetic (no FP on the decode path), the weights are model data
//! charged to `S`, and the trainer is research-plane (`learned-train`).
//!
//! This is a *separate* type family from `crate::learned` on purpose: it is a
//! research-plane corrector and must not perturb the accepted memoryless one,
//! even at the cost of duplicating the small `Net`/`Trainer` pattern.

/// Temporal feature-vector width.
pub const NT: usize = 20;

/// The causal sequence state a temporal step may condition on. Scalars that are
/// bounded by construction (`last_bit`, `prev_pb_ok`) are already in `{-1,0,1}`;
/// the rest are raw integer state that `extract` quantizes.
pub struct TemporalRaw {
    /// Previous coded bit: `+1`, `0` before any bit, or `-1`.
    pub last_bit: i32,
    /// Length of the run of identical recent coded bits.
    pub bit_run: i32,
    /// Consecutive byte-level match predictions that were correct.
    pub match_run: i32,
    /// Smoothed match correctness in `[-2047, 2047]`.
    pub match_ema: i32,
    /// Best repeat-offset confidence in `[-2047, 2047]`.
    pub rep_conf: i32,
    /// The correction this corrector emitted for the previous bit, in stretch
    /// units.
    pub prev_corr: i32,
    /// Stretch of the probability applied to the previous bit.
    pub prev_s: i32,
    /// Stretch of the current pre-correction probability.
    pub s_pr: i32,
    /// Stretch of the mixer output for this bit.
    pub s_raw: i32,
    /// Stretch of the APM2 output for this bit.
    pub s_a2: i32,
    /// Bit position within the current byte, `0..8`.
    pub bitpos: i32,
    /// The most recent coded byte.
    pub last_byte: i32,
    /// Whether the last byte matched the match model's prediction: `+1`/`-1`,
    /// `0` when no match was active.
    pub prev_pb_ok: i32,
    /// Length of the run of identical trailing bytes.
    pub byte_run: i32,
    /// The match-model state for the current bit.
    pub mstate: i32,
    pub word_open: bool,
    pub rep_active: bool,
}

/// The temporal feature vector, quantized to `i8` in `[-32, 31]`.
#[derive(Clone, Copy)]
pub struct TemporalFeats(pub [i8; NT]);

#[inline]
fn q(x: i32) -> i8 {
    x.clamp(-32, 31) as i8
}

/// Extract the temporal feature vector. All features are causal: they depend only
/// on state derived from bytes already coded.
pub fn extract(r: &TemporalRaw) -> TemporalFeats {
    let mut f = [0i8; NT];
    f[0] = r.last_bit.clamp(-1, 1) as i8;
    f[1] = q(r.bit_run);
    f[2] = q(r.match_run);
    f[3] = q(r.match_ema >> 6);
    f[4] = q(r.rep_conf >> 6);
    f[5] = q(r.prev_corr >> 4);
    f[6] = q(r.prev_s >> 6);
    f[7] = q(r.s_pr >> 6);
    f[8] = q(r.bitpos);
    let c = (r.last_byte & 0xff) as u8;
    f[9] = if c.is_ascii_alphabetic() { 1 } else { -1 };
    f[10] = if c.is_ascii_digit() { 1 } else { -1 };
    f[11] = if c == b' ' || c == b'\t' { 1 } else { -1 };
    f[12] = if c == b'\n' { 1 } else { -1 };
    f[13] = r.prev_pb_ok.clamp(-1, 1) as i8;
    f[14] = q(r.byte_run);
    f[15] = q(r.s_raw >> 6);
    f[16] = q(r.s_a2 >> 6);
    f[17] = q(r.mstate - 1);
    f[18] = if r.word_open { 1 } else { -1 };
    f[19] = if r.rep_active { 1 } else { -1 };
    TemporalFeats(f)
}

/// Weight fixed-point scale (a stored `i16` is `value * 256`).
pub const WSCALE: i32 = 256;
/// Clamp on the logit correction, in stretch units.
pub const CORR_CLAMP: i32 = 1024;

/// splitmix64: a deterministic, dependency-free 64-bit mixer for the offline
/// trainer. Gated with the trainer exactly as `crate::learned::splitmix64` is.
#[cfg(feature = "learned-train")]
#[inline]
fn splitmix64(mut x: u64) -> u64 {
    x = x.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = x;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// A frozen, quantized temporal network: one hidden layer of `nh` units.
#[derive(Clone)]
pub struct Temporal {
    pub nh: usize,
    /// `w1[j*NT + i]`, `value * WSCALE`.
    pub w1: Vec<i16>,
    pub b1: Vec<i16>,
    /// `w2[j]`, `value * WSCALE`.
    pub w2: Vec<i16>,
    pub b2: i16,
}

impl Temporal {
    pub fn zero(nh: usize) -> Self {
        Temporal {
            nh,
            w1: vec![0; nh * NT],
            b1: vec![0; nh],
            w2: vec![0; nh],
            b2: 0,
        }
    }

    /// Integer forward pass; returns the logit correction in stretch units.
    ///
    /// Identical arithmetic to `crate::learned::Net::forward`, over `NT` features
    /// rather than `NF`.
    #[inline]
    pub fn predict(&self, f: &TemporalFeats) -> i32 {
        let mut out: i32 = self.b2 as i32;
        for j in 0..self.nh {
            let mut z: i32 = self.b1[j] as i32;
            let row = j * NT;
            for i in 0..NT {
                z += self.w1[row + i] as i32 * f.0[i] as i32;
            }
            let h = (z >> 8).clamp(-127, 127);
            out += self.w2[j] as i32 * h;
        }
        (out >> 8).clamp(-CORR_CLAMP, CORR_CLAMP)
    }

    /// The weight bytes, charged to `S` when this net is embedded.
    pub fn model_bytes(&self) -> u64 {
        (self.w1.len() + self.b1.len() + self.w2.len() + 1) as u64 * 2
    }

    /// Serialize: `magic(4) | nh(u16) | b2(i16) | b1[nh] | w2[nh] | w1[nh*NT]`,
    /// all little-endian `i16`.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut v = Vec::with_capacity(8 + self.model_bytes() as usize);
        v.extend_from_slice(b"ZTP1");
        v.extend_from_slice(&(self.nh as u16).to_le_bytes());
        v.extend_from_slice(&self.b2.to_le_bytes());
        for &x in &self.b1 {
            v.extend_from_slice(&x.to_le_bytes());
        }
        for &x in &self.w2 {
            v.extend_from_slice(&x.to_le_bytes());
        }
        for &x in &self.w1 {
            v.extend_from_slice(&x.to_le_bytes());
        }
        v
    }

    pub fn from_bytes(b: &[u8]) -> Option<Temporal> {
        if b.len() < 8 || &b[0..4] != b"ZTP1" {
            return None;
        }
        let nh = u16::from_le_bytes([b[4], b[5]]) as usize;
        if nh == 0 || nh > 4096 {
            return None;
        }
        let need = 6 + 2 * (1 + nh + nh + nh * NT);
        if b.len() < need {
            return None;
        }
        let mut o = 6usize;
        let mut rd = || {
            let x = i16::from_le_bytes([b[o], b[o + 1]]);
            o += 2;
            x
        };
        let b2 = rd();
        let b1: Vec<i16> = (0..nh).map(|_| rd()).collect();
        let w2: Vec<i16> = (0..nh).map(|_| rd()).collect();
        let w1: Vec<i16> = (0..nh * NT).map(|_| rd()).collect();
        Some(Temporal { nh, w1, b1, w2, b2 })
    }

    /// The mandatory negative control: a deterministic permutation of the weight
    /// arrays that preserves the architecture, model size and code path but
    /// destroys the learned signal. Identical construction to
    /// `crate::learned::Net::shuffled`.
    pub fn shuffled(&self) -> Temporal {
        let shuf = |v: &[i16]| -> Vec<i16> {
            let n = v.len();
            if n == 0 {
                return Vec::new();
            }
            let mut out = vec![0i16; n];
            let k = 7usize;
            let c = 3usize;
            for i in 0..n {
                out[(i * k + c) % n] = v[i];
            }
            out
        };
        Temporal {
            nh: self.nh,
            w1: shuf(&self.w1),
            b1: shuf(&self.b1),
            w2: shuf(&self.w2),
            b2: self.b2.wrapping_add(0x1234),
        }
    }
}

/// Floating-point shadow used only by the offline trainer, mirroring
/// `crate::learned::Trainer`. Research-plane and gated out of the scored build.
#[cfg(feature = "learned-train")]
pub struct TemporalTrainer {
    pub nh: usize,
    pub w1: Vec<f32>,
    pub b1: Vec<f32>,
    pub w2: Vec<f32>,
    pub b2: f32,
    pub lr: f32,
    pub steps: u64,
    pub loss_bits: f64,
    pub ema_bits: f64,
}

#[cfg(feature = "learned-train")]
impl TemporalTrainer {
    pub fn new(nh: usize, lr: f32) -> Self {
        // A different seed from the memoryless trainer, so the two are not
        // accidentally correlated initialisations.
        let mut s: u64 = 0x7A17_5EED_C0DE_1234;
        let mut rng = || -> f32 {
            s = splitmix64(s);
            ((s >> 40) as f32 / (1u64 << 24) as f32) - 0.5
        };
        let w1 = (0..nh * NT).map(|_| rng() * 0.2).collect();
        let b1 = vec![0.0; nh];
        let w2 = (0..nh).map(|_| rng() * 0.2).collect();
        TemporalTrainer {
            nh,
            w1,
            b1,
            w2,
            b2: 0.0,
            lr,
            steps: 0,
            loss_bits: 0.0,
            ema_bits: 0.0,
        }
    }

    #[inline]
    fn lr_now(&self) -> f32 {
        self.lr / (1.0 + self.steps as f32 / 2_000_000.0)
    }

    #[inline]
    fn forward_raw(&self, f: &TemporalFeats) -> (Vec<f32>, f32) {
        let mut h = vec![0.0f32; self.nh];
        for j in 0..self.nh {
            let mut z = self.b1[j];
            let row = j * NT;
            for i in 0..NT {
                z += self.w1[row + i] * f.0[i] as f32;
            }
            h[j] = z.clamp(-127.0, 127.0);
        }
        let mut out = self.b2;
        for j in 0..self.nh {
            out += self.w2[j] * h[j];
        }
        (h, out.clamp(-(CORR_CLAMP as f32), CORR_CLAMP as f32))
    }

    /// One SGD step. `s_pr` is the pre-correction stretched logit; `bit` the
    /// observed bit. Returns the applied loss in bits.
    #[inline]
    pub fn step(&mut self, f: &TemporalFeats, s_pr: i32, bit: u32) -> f64 {
        let (h, out) = self.forward_raw(f);
        let corr = out.round() as i32;
        let d = (s_pr + corr).clamp(-2047, 2047);
        let p = crate::mixer::squash(d) as f32 / 4096.0;
        let y = bit as f32;
        // dL/dcorr in stretch units. 257 stretch units ~ 1 natural logit.
        let dl = (p - y) / 257.0;
        let dout = if out.abs() < CORR_CLAMP as f32 {
            dl
        } else {
            0.0
        };
        let lr = self.lr_now();
        for j in 0..self.nh {
            let g = dout * h[j];
            self.w2[j] -= lr * g;
        }
        self.b2 -= lr * dout;
        for j in 0..self.nh {
            let dz = if h[j].abs() < 127.0 {
                dout * self.w2[j]
            } else {
                0.0
            };
            let row = j * NT;
            for i in 0..NT {
                self.w1[row + i] -= lr * dz * f.0[i] as f32;
            }
            self.b1[j] -= lr * dz;
        }
        const WMAX: f32 = 4.0;
        for w in self.w1.iter_mut() {
            *w = w.clamp(-WMAX, WMAX);
        }
        for w in self.w2.iter_mut() {
            *w = w.clamp(-WMAX, WMAX);
        }
        for w in self.b1.iter_mut() {
            *w = w.clamp(-WMAX, WMAX);
        }
        self.b2 = self.b2.clamp(-WMAX, WMAX);
        self.steps += 1;
        let pb = p.clamp(1e-9, 1.0 - 1e-9);
        let bits = if bit != 0 {
            -pb.log2()
        } else {
            -(1.0 - pb).log2()
        };
        self.loss_bits += bits as f64;
        let a = 0.00005f64;
        self.ema_bits = if self.steps == 1 {
            bits as f64
        } else {
            (1.0 - a) * self.ema_bits + a * bits as f64
        };
        bits as f64
    }

    /// The rounded correction, for the training forward pass (mirrors
    /// `Temporal::predict` closely enough that the quantized net behaves as
    /// trained).
    #[inline]
    pub fn corr_for(&self, f: &TemporalFeats) -> i32 {
        let (_, out) = self.forward_raw(f);
        out.round() as i32
    }

    /// Quantize to the integer network with `i16` weights.
    pub fn quantize(&self) -> Temporal {
        let qw = |x: f32| -> i16 { (x * WSCALE as f32).round().clamp(-32767.0, 32767.0) as i16 };
        Temporal {
            nh: self.nh,
            w1: self.w1.iter().map(|&x| qw(x)).collect(),
            b1: self.b1.iter().map(|&x| qw(x)).collect(),
            w2: self.w2.iter().map(|&x| qw(x)).collect(),
            b2: qw(self.b2),
        }
    }

    pub fn mean_loss_bits(&self) -> f64 {
        if self.steps == 0 {
            0.0
        } else {
            self.loss_bits / self.steps as f64
        }
    }
}

/// The frozen, offline-trained temporal weights, embedded in the scored binary.
/// Generated by `zentropy train-temporal`; an empty file disables the corrector.
pub const WEIGHTS: &[u8] = include_bytes!("temporal.bin");

/// Load the embedded temporal network, if any.
#[inline]
pub fn load() -> Option<Temporal> {
    Temporal::from_bytes(WEIGHTS)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn feats() -> TemporalFeats {
        TemporalFeats([
            1, -2, 3, 0, 4, 1, 2, -1, 1, -1, 5, -3, 1, 2, -1, 1, 0, 3, 1, -1,
        ])
    }

    #[test]
    fn zero_net_is_neutral() {
        let n = Temporal::zero(8);
        assert_eq!(n.predict(&feats()), 0);
    }

    #[test]
    fn serialization_roundtrips() {
        let mut n = Temporal::zero(4);
        n.w1[0] = 1234;
        n.b2 = -77;
        n.w2[3] = 9;
        let b = n.to_bytes();
        let m = Temporal::from_bytes(&b).unwrap();
        assert_eq!(m.nh, 4);
        assert_eq!(m.w1[0], 1234);
        assert_eq!(m.b2, -77);
        assert_eq!(m.w2[3], 9);
        assert!(Temporal::from_bytes(b"short").is_none());
        assert!(Temporal::from_bytes(b"ZTP1").is_none());
    }

    #[test]
    fn shuffled_changes_weights_but_keeps_size() {
        let mut n = Temporal::zero(6);
        for (i, x) in n.w1.iter_mut().enumerate() {
            *x = i as i16;
        }
        let s = n.shuffled();
        assert_eq!(s.model_bytes(), n.model_bytes());
        assert_ne!(s.w1, n.w1);
    }

    #[test]
    fn extract_is_causal_and_bounded() {
        let f = extract(&TemporalRaw {
            last_bit: -1,
            bit_run: 1000,
            match_run: 4,
            match_ema: 2047,
            rep_conf: -2047,
            prev_corr: 1024,
            prev_s: 900,
            s_pr: -900,
            s_raw: 2047,
            s_a2: -2047,
            bitpos: 7,
            last_byte: b'\n' as i32,
            prev_pb_ok: 1,
            byte_run: 500,
            mstate: 3,
            word_open: true,
            rep_active: false,
        });
        assert!(f.0.iter().all(|&x| (-32..=31).contains(&(x as i32))));
        assert_eq!(f.0[12], 1); // newline
        assert_eq!(f.0[19], -1); // no repeat
    }

    #[test]
    #[cfg(feature = "learned-train")]
    fn embedded_int_net_matches_dequantized_float() {
        // If this fails, the integer runtime and the float trainer disagree and
        // the trained temporal network cannot be shipped.
        let Some(n) = load() else {
            return;
        };
        let mut s = 0xC0FF_EE00u64;
        let mut maxdiff = 0i32;
        let mut worst = String::new();
        for _ in 0..400 {
            let mut f = [0i8; NT];
            for x in f.iter_mut() {
                s = splitmix64(s);
                *x = ((s % 64) as i32 - 32) as i8;
            }
            let f = TemporalFeats(f);
            let intc = n.predict(&f);
            let mut h = vec![0f32; n.nh];
            for j in 0..n.nh {
                let mut z = n.b1[j] as f32 / WSCALE as f32;
                for i in 0..NT {
                    z += (n.w1[j * NT + i] as f32 / WSCALE as f32) * f.0[i] as f32;
                }
                h[j] = z.clamp(-127.0, 127.0);
            }
            let mut out = n.b2 as f32 / WSCALE as f32;
            for j in 0..n.nh {
                out += (n.w2[j] as f32 / WSCALE as f32) * h[j];
            }
            let fl = out.clamp(-(CORR_CLAMP as f32), CORR_CLAMP as f32).round() as i32;
            let d = (intc - fl).abs();
            if d > maxdiff {
                maxdiff = d;
                worst = format!("f={:?} int={intc} fl={fl}", f.0);
            }
        }
        let mw1 = n.w1.iter().map(|x| x.abs()).max().unwrap_or(0);
        let mw2 = n.w2.iter().map(|x| x.abs()).max().unwrap_or(0);
        // The bound is DERIVED, not a magic constant. The integer forward shifts
        // both accumulators toward negative infinity while the float reference
        // rounds, so each hidden unit may differ by up to one and the output by up
        // to one plus the `w2`-weighted sum of those unit errors, i.e.
        // `1 + sum|w2| / WSCALE`. Deriving it from the shipped weights keeps the
        // test's real purpose -- catching gross trainer/inference divergence --
        // while letting the rounding convention be a property of the weights
        // rather than of the assertion.
        //
        // This test is not decorative: it is what caught the Phase 14.34 finding
        // that a WIDER net quantizes worse. The enwik9 h=8 net measures 10 against
        // a derived bound of 33 (its weights sit at the trainer's +-4.0 clamp, so
        // the unit errors are the largest the scheme permits); the enwik8 h=16 net
        // measured 5,442 B worse in archive at the SAME reported training loss,
        // which is exactly the divergence this figure bounds. Driving it down is
        // the weight-quantization work of section 14.35, not a loosened assertion.
        let l1_w2: i64 = n.w2.iter().map(|x| (*x as i64).abs()).sum();
        let bound = 1 + (l1_w2 / WSCALE as i64) as i32;
        assert!(
            maxdiff <= bound,
            "integer/float temporal mismatch {maxdiff} > derived bound {bound} nh={} max|w1|={mw1} max|w2|={mw2} b2={} worst: {worst}",
            n.nh,
            n.b2
        );
    }

    #[test]
    #[cfg(feature = "learned-train")]
    fn trainer_learns_a_constant_bias() {
        let f = TemporalFeats([0; NT]);
        let mut t = TemporalTrainer::new(4, 5.0);
        for _ in 0..50_000 {
            t.step(&f, 0, 1);
        }
        let net = t.quantize();
        assert!(net.predict(&f) > 0, "correction did not become positive");
    }

    /// Diagnostic (run with `--ignored --nocapture`): the weight statistics of the
    /// two preserved training artefacts. It exists because the enwik9-trained net
    /// *hurts* the archive it was trained on (+1,566,863 B at enwik9, +155,752 B at
    /// enwik8) while the enwik8-trained net helps (-10,827 B at enwik8), and the
    /// question is whether the longer training simply saturated the weights.
    #[test]
    #[ignore]
    fn dump_preserved_weight_stats() {
        for p in [
            "evidence/phase14/temporal/enwik8-h8.bin",
            "evidence/phase14/temporal/enwik9-h8.bin",
        ] {
            let Ok(b) = std::fs::read(p) else {
                continue;
            };
            let Some(n) = Temporal::from_bytes(&b) else {
                println!("{p}: not a temporal net");
                continue;
            };
            let mi = |v: &[i16]| v.iter().map(|x| x.unsigned_abs()).max().unwrap_or(0);
            let sat = |v: &[i16]| {
                v.iter()
                    .filter(|x| x.unsigned_abs() as i32 >= WSCALE * 4 - 2)
                    .count()
            };
            let sum_w2: i64 = n.w2.iter().map(|x| *x as i64).sum();
            println!(
                "{p}: nh={} max|b1|={} max|w2|={} max|w1|={} sat_w1={} sat_w2={} b2={} sum_w2={}",
                n.nh,
                mi(&n.b1),
                mi(&n.w2),
                mi(&n.w1),
                sat(&n.w1),
                sat(&n.w2),
                n.b2,
                sum_w2
            );
        }
    }
}
