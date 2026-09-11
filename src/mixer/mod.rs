//! Context-mixing primitives: logistic transforms, an adaptive mixer and
//! secondary estimation.
//!
//! These are the mechanisms that actually turn a pile of context models into a
//! probability (§6, §11.6). The design is deliberately small and inspectable:
//! every expert reports a prediction and is mixed in the logistic domain, and
//! the composite is passed through calibration stages before the range coder.
//!
//! Nothing here is novel. Novelty is not the goal; measurable `ΔS < 0` is.

/// Logistic squash: map a stretched value in `[-2047, 2047]` to a 12-bit
/// probability in `[0, 4095]`. Uses the standard PAQ piecewise table.
#[inline]
pub fn squash(d: i32) -> i32 {
    const T: [i32; 33] = [
        1, 2, 3, 6, 10, 16, 27, 45, 73, 120, 194, 310, 488, 747, 1101, 1546, 2047, 2549, 2994,
        3348, 3607, 3785, 3901, 3975, 4024, 4050, 4068, 4079, 4085, 4089, 4092, 4093, 4094,
    ];
    if d > 2047 {
        return 4095;
    }
    if d < -2047 {
        return 0;
    }
    let w = d & 127;
    let d = (d >> 7) + 16; // 0..32
    (T[d as usize] * (128 - w) + T[d as usize + 1] * w + 64) >> 7
}

/// A precomputed `stretch` table: the inverse of [`squash`], 4096 entries.
pub struct StretchTable {
    t: [i16; 4096],
}

impl StretchTable {
    pub fn new() -> Self {
        let mut t = [0i16; 4096];
        let mut pi = 0usize;
        for x in -2047..=2047i32 {
            let p = squash(x) as usize;
            let mut j = pi;
            while j <= p && j < 4096 {
                t[j] = x as i16;
                j += 1;
            }
            pi = p + 1;
        }
        while pi < 4096 {
            t[pi] = 2047;
            pi += 1;
        }
        StretchTable { t }
    }

    #[inline]
    pub fn stretch(&self, p: i32) -> i32 {
        self.t[p.clamp(0, 4095) as usize] as i32
    }
}

impl Default for StretchTable {
    fn default() -> Self {
        Self::new()
    }
}

/// An adaptive logistic mixer over `n` expert predictions, with one weight
/// vector per context. This is the "single layer neural network selected by a
/// small context" of the PAQ/lpaq lineage.
#[derive(Debug, Clone)]
pub struct Mixer {
    /// `weights[cx * n + i]`, 16.16 fixed point (65536 == weight 1.0).
    weights: Vec<i32>,
    n: usize,
    n_ctx: usize,
    /// Currently selected context and cached inputs.
    cx: usize,
    inputs: Vec<i32>,
    pr: i32,
    /// Learning-rate multiplier (fixed point, 16 == 1.0).
    lr: i32,
}

impl Mixer {
    /// `n` experts, `n_ctx` weight sets.
    pub fn new(n: usize, n_ctx: usize) -> Self {
        // Initialise weights to 1/n * 65536 so the initial mix is an average.
        let init = (65536 / n.max(1) as i32).max(1);
        Mixer {
            weights: vec![init; n * n_ctx],
            n,
            n_ctx,
            cx: 0,
            inputs: vec![0; n],
            pr: 2048,
            lr: 12,
        }
    }

    pub fn set_learning_rate(&mut self, lr: i32) {
        self.lr = lr;
    }

    /// Mix the expert predictions (already stretched to `[-2047, 2047]`) and
    /// select the weight set `cx`.
    #[inline]
    pub fn mix(&mut self, inputs: &[i32], cx: usize) -> i32 {
        debug_assert_eq!(inputs.len(), self.n);
        self.inputs.copy_from_slice(inputs);
        self.cx = cx % self.n_ctx;
        let base = self.cx * self.n;
        let mut dot: i64 = 0;
        for i in 0..self.n {
            dot += (self.weights[base + i] as i64) * (inputs[i] as i64);
        }
        let d = (dot >> 16) as i32;
        self.pr = squash(d.clamp(-2047, 2047));
        self.pr
    }

    /// Update the selected weight set toward the observed bit.
    #[inline]
    pub fn update(&mut self, y: u32) {
        let err = ((y as i32) << 12) - self.pr; // in [-4095, 4095]
        let base = self.cx * self.n;
        for i in 0..self.n {
            // 16.16 weight update: w += lr * err * st / 2^16
            let delta = ((self.inputs[i] as i64) * (err as i64) * (self.lr as i64)) >> 16;
            self.weights[base + i] += delta as i32;
        }
    }

    pub fn n_inputs(&self) -> usize {
        self.n
    }
}

/// Adaptive probability map / secondary symbol estimation (SSE), after the PAQ
/// APM component. Interpolates a 33-entry table per context on the stretched
/// prediction and adapts each entry toward the observed bit.
#[derive(Debug, Clone)]
pub struct Apm {
    t: Vec<i32>,
    n: usize,
    /// Index of the lower interpolation entry chosen by the last `predict`.
    idx: usize,
    rate: u32,
}

impl Apm {
    /// `n` contexts. `rate` is the adaptation shift (larger = slower).
    pub fn new(n: usize, rate: u32) -> Self {
        let mut t = vec![0i32; n * 33];
        for c in 0..n {
            for j in 0..33 {
                // Initial map is the identity logistic curve.
                t[c * 33 + j] = squash((j as i32 - 16) * 128) << 4;
            }
        }
        Apm { t, n, idx: 0, rate }
    }

    /// Map a 12-bit prediction through context `cxt`, returning a 12-bit
    /// prediction. Call [`Apm::update`] with the coded bit afterwards.
    #[inline]
    pub fn predict(&mut self, pr: i32, cxt: usize) -> i32 {
        let cxt = cxt % self.n;
        let w = (pr & 127) as i64;
        let hi = (pr >> 7) as usize; // 0..31
        let i = cxt * 33 + hi;
        self.idx = i;
        let v = (self.t[i] as i64 * (128 - w) + self.t[i + 1] as i64 * w) >> 11;
        v.clamp(0, 4095) as i32
    }

    #[inline]
    pub fn update(&mut self, y: u32) {
        let g: i32 = if y != 0 {
            (1 << 16) + (1 << self.rate) - 2
        } else {
            0
        };
        let i = self.idx;
        self.t[i] += (g - self.t[i]) >> self.rate;
        self.t[i + 1] += (g - self.t[i + 1]) >> self.rate;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn squash_stretch_are_inverses() {
        let s = StretchTable::new();
        for p in [1, 100, 1000, 2048, 3000, 4000, 4094] {
            let x = s.stretch(p);
            let back = squash(x);
            assert!((back - p).abs() <= 128, "p={p} stretch={x} squash={back}");
        }
    }

    #[test]
    fn squash_monotone_and_bounded() {
        let mut prev = -1;
        for x in -2047..=2047 {
            let p = squash(x);
            assert!((0..=4095).contains(&p));
            assert!(p >= prev, "not monotone at x={x}");
            prev = p;
        }
        assert_eq!(squash(100000), 4095);
        assert_eq!(squash(-100000), 0);
    }

    #[test]
    fn mixer_learns_a_constant() {
        // One expert that always says "1" with high confidence; the mixer must
        // learn to trust it.
        let s = StretchTable::new();
        let mut m = Mixer::new(2, 1);
        for _ in 0..20_000 {
            let st1 = s.stretch(3800);
            let st2 = s.stretch(300);
            let p = m.mix(&[st1, st2], 0);
            m.update(1);
            let _ = p;
        }
        let st1 = s.stretch(3800);
        let st2 = s.stretch(300);
        let p = m.mix(&[st1, st2], 0);
        assert!(p > 3000, "mixer failed to learn: p={p}");
    }

    #[test]
    fn mixer_learns_the_opposite() {
        let s = StretchTable::new();
        let mut m = Mixer::new(2, 1);
        for _ in 0..20_000 {
            let st1 = s.stretch(3800);
            let st2 = s.stretch(300);
            let _ = m.mix(&[st1, st2], 0);
            m.update(0);
        }
        let st1 = s.stretch(3800);
        let st2 = s.stretch(300);
        let p = m.mix(&[st1, st2], 0);
        assert!(p < 1000, "mixer failed to invert: p={p}");
    }

    #[test]
    fn apm_refines_a_biased_prediction() {
        let mut apm = Apm::new(1, 8);
        // Feed pr=2048 but the true bit is always 1; APM should drift up.
        for _ in 0..50_000 {
            let q = apm.predict(2048, 0);
            let _ = q;
            apm.update(1);
        }
        let q = apm.predict(2048, 0);
        assert!(q > 2600, "apm did not calibrate: q={q}");
    }
}
