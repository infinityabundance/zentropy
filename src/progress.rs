//! Progress reporting for long runs.
//!
//! A context-mixing pass over enwik9 takes tens of minutes at ~0.5 MB/s, and
//! "tens of minutes with no output" is indistinguishable from a hang. Every
//! coding loop therefore reports a rate-limited status line on stderr: percent,
//! bytes done, throughput, elapsed and ETA.
//!
//! Compiled only under `--features progress`, which is in the research default
//! but **not** in `accepted`, so the scored stub is built without any of it.

use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

/// How often a status line is allowed to be printed.
const INTERVAL: Duration = Duration::from_millis(2000);

static ENABLED: AtomicBool = AtomicBool::new(true);

/// Turn progress output on or off globally.
///
/// The driver silences it around *concurrent* encodes, where several passes
/// would interleave their carriage-return lines into nonsense.
pub fn set(on: bool) {
    ENABLED.store(on, Ordering::Relaxed);
}

/// Whether progress output is currently on.
pub fn enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

/// A rate-limited progress line for one pass over `total` bytes.
pub struct Progress {
    label: &'static str,
    total: u64,
    start: Instant,
    last: Instant,
    active: bool,
}

impl Progress {
    /// Begin reporting for a pass. Prints a starting line so a run is visibly
    /// alive before the first interval has elapsed.
    pub fn new(label: &'static str, total: u64) -> Self {
        let now = Instant::now();
        let active = enabled() && total > 0;
        if active {
            let _ = writeln!(
                std::io::stderr(),
                "[{label}] starting: {} to code",
                human(total)
            );
        }
        Progress {
            label,
            total,
            start: now,
            last: now,
            active,
        }
    }

    /// Report progress at `done` bytes. Prints at most once per [`INTERVAL`],
    /// and always on the final byte.
    pub fn tick(&mut self, done: u64) {
        if !self.active || !enabled() {
            return;
        }
        let now = Instant::now();
        if done < self.total && now.duration_since(self.last) < INTERVAL {
            return;
        }
        self.last = now;
        let elapsed = now.duration_since(self.start).as_secs_f64();
        let frac = (done as f64 / self.total as f64).min(1.0);
        let rate = if elapsed > 0.0 {
            done as f64 / elapsed
        } else {
            0.0
        };
        let eta = if rate > 0.0 {
            self.total.saturating_sub(done) as f64 / rate
        } else {
            0.0
        };
        let mut err = std::io::stderr();
        let _ = write!(
            err,
            "\r[{}] {:>5.1}%  {} / {}  {:.2} MB/s  elapsed {}  ETA {}      ",
            self.label,
            frac * 100.0,
            human(done),
            human(self.total),
            rate / (1024.0 * 1024.0),
            clock(elapsed),
            clock(eta),
        );
        let _ = err.flush();
    }

    /// Finish the pass and start a fresh line.
    pub fn finish(&mut self) {
        if !self.active {
            return;
        }
        self.active = false;
        let elapsed = self.start.elapsed().as_secs_f64();
        let _ = writeln!(
            std::io::stderr(),
            "\r[{}] 100.0%  {}  done in {}",
            self.label,
            human(self.total),
            clock(elapsed)
        );
    }
}

impl Drop for Progress {
    fn drop(&mut self) {
        // A pass that returned early (an error path) still ends its line, so the
        // next message does not appear inside a progress line.
        self.finish();
    }
}

fn human(b: u64) -> String {
    const GIB: f64 = 1024.0 * 1024.0 * 1024.0;
    const MIB: f64 = 1024.0 * 1024.0;
    const KIB: f64 = 1024.0;
    let f = b as f64;
    if f >= GIB {
        format!("{:.2} GB", f / GIB)
    } else if f >= MIB {
        format!("{:.1} MB", f / MIB)
    } else if f >= KIB {
        format!("{:.1} KB", f / KIB)
    } else {
        format!("{b} B")
    }
}

fn clock(secs: f64) -> String {
    let s = if secs.is_finite() {
        secs.max(0.0) as u64
    } else {
        0
    };
    if s >= 3600 {
        format!("{}h{:02}m{:02}s", s / 3600, (s % 3600) / 60, s % 60)
    } else if s >= 60 {
        format!("{}m{:02}s", s / 60, s % 60)
    } else {
        format!("{s}s")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_and_clocks_render() {
        assert_eq!(human(512), "512 B");
        assert_eq!(human(2048), "2.0 KB");
        assert_eq!(human(3 * 1024 * 1024), "3.0 MB");
        assert_eq!(clock(45.0), "45s");
        assert_eq!(clock(125.0), "2m05s");
        assert_eq!(clock(3725.0), "1h02m05s");
        // A non-finite rate must not produce a nonsense clock.
        assert_eq!(clock(f64::INFINITY), "0s");
    }

    #[test]
    fn disabled_progress_is_silent_and_safe() {
        set(false);
        let mut p = Progress::new("test", 100);
        p.tick(50);
        p.finish();
        assert!(!enabled());
        set(true);
        assert!(enabled());
    }
}
