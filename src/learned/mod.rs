//! Phase 8 — the learned residual corrector.
//!
//! The classical chain (mixer -> APM1 -> APM2 -> APM3) already produces a
//! calibrated probability. This module adds a small, **offline-trained**,
//! quantized network that consumes the *classical outputs as features* and emits
//! a logit correction — the T2 cascade: the learned capacity is spent on the
//! residual the classical model leaves, never on re-learning text from scratch.
//!
//! Scored-path properties:
//!
//! * inference is fixed-point integer arithmetic (deterministic, no FP on the
//!   decode path);
//! * the weights are model data and their bytes are charged to `S`;
//! * training is research-plane only and lives behind the `learned` feature.

use crate::mixer::{squash, StretchTable};

/// Feature-vector width.
pub const NF: usize = 12;

/// The classical outputs and state a residual step may condition on. All
/// `s_*` values are already stretched logits in `[-2047, 2047]`.
pub struct Raw14 {
    pub s_raw: i32,
    pub s_a1: i32,
    pub s_a2: i32,
    pub s_a3: i32,
    pub s_pr: i32,
    pub mstate: i32,
    /// +1/-1 if a match is active and predicts a 1/0 next bit, else 0.
    pub match_dir: i32,
    pub bitpos: i32,
    pub word_open: bool,
    pub rep_active: bool,
}

/// The feature vector, quantized to `i8` (values fit comfortably in `[-32, 31]`).
#[derive(Clone, Copy)]
pub struct Feats(pub [i8; NF]);

#[inline]
fn q(x: i32) -> i8 {
    x.clamp(-32, 31) as i8
}

/// Extract the classical-residual feature vector.
pub fn extract(r: &Raw14) -> Feats {
    let mut f = [0i8; NF];
    f[0] = q(r.s_raw >> 6);
    f[1] = q(r.s_a1 >> 6);
    f[2] = q(r.s_a2 >> 6);
    f[3] = q(r.s_a3 >> 6);
    f[4] = q(r.s_pr >> 6);
    f[5] = q(r.mstate - 1);
    f[6] = (r.match_dir.clamp(-1, 1) * 2) as i8;
    f[7] = q(r.bitpos - 4);
    f[8] = if r.word_open { 1 } else { -1 };
    f[9] = if r.rep_active { 1 } else { -1 };
    f[10] = q((r.s_a2 - r.s_a1) >> 6);
    f[11] = q((r.s_raw - r.s_pr) >> 6);
    Feats(f)
}

/// Weight fixed-point scale (a stored `i16` is `value * 256`).
pub const WSCALE: i32 = 256;
/// Clamp on the logit correction, in stretch units.
pub const CORR_CLAMP: i32 = 1024;

/// splitmix64: a deterministic, dependency-free 64-bit mixer.
#[inline]
fn splitmix64(mut x: u64) -> u64 {
    x = x.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = x;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// A frozen, quantized residual network: one hidden layer of `nh` units.
#[derive(Clone)]
pub struct Net {
    pub nh: usize,
    /// `w1[j*NF + i]`, `value * WSCALE`.
    pub w1: Vec<i16>,
    pub b1: Vec<i16>,
    /// `w2[j]`, `value * WSCALE`.
    pub w2: Vec<i16>,
    pub b2: i16,
}

impl Net {
    pub fn zero(nh: usize) -> Self {
        Net {
            nh,
            w1: vec![0; nh * NF],
            b1: vec![0; nh],
            w2: vec![0; nh],
            b2: 0,
        }
    }

    /// Integer forward pass; returns the logit correction in stretch units.
    ///
    /// Weights are stored as `value * WSCALE`, so an accumulator is `WSCALE`
    /// times the real pre-activation; the `>> 8` shifts undo that. Biases are in
    /// the same stored units and must NOT be scaled again.
    #[inline]
    pub fn forward(&self, f: &Feats) -> i32 {
        let mut out: i32 = self.b2 as i32;
        for j in 0..self.nh {
            let mut z: i32 = self.b1[j] as i32;
            let row = j * NF;
            for i in 0..NF {
                z += self.w1[row + i] as i32 * f.0[i] as i32;
            }
            let h = (z >> 8).clamp(-127, 127);
            out += self.w2[j] as i32 * h;
        }
        (out >> 8).clamp(-CORR_CLAMP, CORR_CLAMP)
    }

    pub fn model_bytes(&self) -> u64 {
        (self.w1.len() + self.b1.len() + self.w2.len() + 1) as u64 * 2
    }

    /// Serialize: `magic(4) | nh(u16) | b2(i16) | b1[nh] | w2[nh] | w1[nh*NF]`,
    /// all little-endian `i16`.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut v = Vec::with_capacity(8 + self.model_bytes() as usize);
        v.extend_from_slice(b"ZRN1");
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

    pub fn from_bytes(b: &[u8]) -> Option<Net> {
        if b.len() < 8 || &b[0..4] != b"ZRN1" {
            return None;
        }
        let nh = u16::from_le_bytes([b[4], b[5]]) as usize;
        if nh == 0 || nh > 4096 {
            return None;
        }
        let need = 6 + 2 * (1 + nh + nh + nh * NF);
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
        let w1: Vec<i16> = (0..nh * NF).map(|_| rd()).collect();
        Some(Net { nh, w1, b1, w2, b2 })
    }

    /// Phase 8.7 control: a deterministic permutation of the weight arrays that
    /// preserves the architecture, model size and code path but destroys the
    /// learned signal.
    pub fn shuffled(&self) -> Net {
        let shuf = |v: &[i16]| -> Vec<i16> {
            let n = v.len();
            if n == 0 {
                return Vec::new();
            }
            let mut out = vec![0i16; n];
            // A fixed derangement by index: out[(i*k+c) % n] = v[i].
            let k = 7usize;
            let c = 3usize;
            for i in 0..n {
                out[(i * k + c) % n] = v[i];
            }
            out
        };
        Net {
            nh: self.nh,
            w1: shuf(&self.w1),
            b1: shuf(&self.b1),
            w2: shuf(&self.w2),
            b2: self.b2.wrapping_add(0x1234),
        }
    }
}

/// Floating-point shadow used only by the offline trainer. The trainer mirrors
/// the integer forward pass so the quantized network it produces behaves as it
/// was trained.
pub struct Trainer {
    pub nh: usize,
    pub w1: Vec<f32>,
    pub b1: Vec<f32>,
    pub w2: Vec<f32>,
    pub b2: f32,
    pub lr: f32,
    pub steps: u64,
    /// Running log loss in bits per bit, for the training receipt.
    pub loss_bits: f64,
    /// Exponential moving average of the recent loss (divergence detector).
    pub ema_bits: f64,
}

impl Trainer {
    pub fn new(nh: usize, lr: f32) -> Self {
        // Small deterministic random weights: a zero-initialised hidden layer has
        // zero gradient through `w1`, which would leave only a constant bias.
        let mut s: u64 = 0x1234_5678_9ABC_DEF0;
        let mut rng = || -> f32 {
            s = splitmix64(s);
            ((s >> 40) as f32 / (1u64 << 24) as f32) - 0.5
        };
        let w1 = (0..nh * NF).map(|_| rng() * 0.2).collect();
        let b1 = vec![0.0; nh];
        let w2 = (0..nh).map(|_| rng() * 0.2).collect();
        Trainer {
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

    /// Decaying learning rate and a hard weight bound keep the online trainer
    /// stable over hundreds of millions of steps.
    #[inline]
    fn lr_now(&self) -> f32 {
        self.lr / (1.0 + self.steps as f32 / 2_000_000.0)
    }

    /// Forward in `f32`; returns `(hidden, correction)`.
    #[inline]
    fn forward_raw(&self, f: &Feats) -> (Vec<f32>, f32) {
        let mut h = vec![0.0f32; self.nh];
        for j in 0..self.nh {
            let mut z = self.b1[j];
            let row = j * NF;
            for i in 0..NF {
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
    /// observed bit. Returns the applied probability loss in bits (so training
    /// can be tracked without floating-log tables).
    #[inline]
    pub fn step(&mut self, f: &Feats, s_pr: i32, bit: u32) -> f64 {
        let (h, out) = self.forward_raw(f);
        let corr = out.round() as i32;
        let d = (s_pr + corr).clamp(-2047, 2047);
        let p = squash(d) as f32 / 4096.0;
        let y = bit as f32;
        // dL/dcorr in stretch units. 257 stretch units ~ 1 natural logit.
        let dl = (p - y) / 257.0;
        // Clamp the correction's gradient only when the output saturated.
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
            let row = j * NF;
            for i in 0..NF {
                self.w1[row + i] -= lr * dz * f.0[i] as f32;
            }
            self.b1[j] -= lr * dz;
        }
        // Bound every weight: an unbounded online corrector diverges.
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

    /// The rounded correction, for the training forward pass (mirrors `Net::forward`
    /// closely enough that the quantized net behaves as trained).
    #[inline]
    pub fn corr_for(&self, f: &Feats) -> i32 {
        let (_, out) = self.forward_raw(f);
        out.round() as i32
    }

    /// Quantize to the integer network with `i16` weights (T9).
    pub fn quantize(&self) -> Net {
        let qw = |x: f32| -> i16 { (x * WSCALE as f32).round().clamp(-32767.0, 32767.0) as i16 };
        Net {
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

/// Convenience for the predictor: predict the correction with either path.
#[inline]
pub fn stretch_and_apply(st: &StretchTable, pr: i32, corr: i32) -> i32 {
    squash((st.stretch(pr) + corr).clamp(-2047, 2047))
}

/// The frozen, offline-trained weights, embedded in the scored binary. The file
/// is generated by `zentropy train-residual`; an empty file disables the
/// corrector.
pub const WEIGHTS: &[u8] = include_bytes!("weights.bin");

/// Load the embedded network, if any.
#[inline]
pub fn load() -> Option<Net> {
    Net::from_bytes(WEIGHTS)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn feats() -> Feats {
        Feats([1, -2, 3, 0, 4, 1, 2, -1, 1, -1, 5, -3])
    }

    #[test]
    fn zero_net_is_neutral() {
        let n = Net::zero(8);
        assert_eq!(n.forward(&feats()), 0);
    }

    #[test]
    fn serialization_roundtrips() {
        let mut n = Net::zero(4);
        n.w1[0] = 1234;
        n.b2 = -77;
        n.w2[3] = 9;
        let b = n.to_bytes();
        let m = Net::from_bytes(&b).unwrap();
        assert_eq!(m.nh, 4);
        assert_eq!(m.w1[0], 1234);
        assert_eq!(m.b2, -77);
        assert_eq!(m.w2[3], 9);
        assert!(Net::from_bytes(b"short").is_none());
    }

    #[test]
    fn shuffled_changes_weights_but_keeps_size() {
        let mut n = Net::zero(6);
        for (i, x) in n.w1.iter_mut().enumerate() {
            *x = i as i16;
        }
        let s = n.shuffled();
        assert_eq!(s.model_bytes(), n.model_bytes());
        assert_ne!(s.w1, n.w1);
    }

    #[test]
    fn embedded_int_net_matches_dequantized_float() {
        // If this fails, the integer runtime and the float trainer disagree and
        // the trained network cannot be shipped.
        let Some(n) = load() else {
            return;
        };
        let mut s = 0xDEAD_BEEFu64;
        let mut maxdiff = 0i32;
        let mut worst = String::new();
        for _ in 0..400 {
            let mut f = [0i8; NF];
            for x in f.iter_mut() {
                s = splitmix64(s);
                *x = ((s % 64) as i32 - 32) as i8;
            }
            let f = Feats(f);
            let intc = n.forward(&f);
            let mut h = vec![0f32; n.nh];
            for j in 0..n.nh {
                let mut z = n.b1[j] as f32 / WSCALE as f32;
                for i in 0..NF {
                    z += (n.w1[j * NF + i] as f32 / WSCALE as f32) * f.0[i] as f32;
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
        assert!(
            maxdiff <= 8,
            "integer/float residual mismatch {maxdiff} nh={} max|w1|={mw1} max|w2|={mw2} b2={} worst: {worst}",
            n.nh,
            n.b2
        );
    }

    #[test]
    fn trainer_learns_a_constant_bias() {
        // With zero features and a target bit always 1, the corrector must learn
        // a positive correction (the bounded global bias reaches +4).
        let f = Feats([0; NF]);
        let mut t = Trainer::new(4, 5.0);
        for _ in 0..50_000 {
            t.step(&f, 0, 1);
        }
        let net = t.quantize();
        assert!(net.forward(&f) > 0, "correction did not become positive");
    }
}
