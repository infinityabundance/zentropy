//! The P14.9 report (§14.52 content), printed by the command.
//!
//! Every number is labelled: `A` is the accepted configuration's real container
//! bytes for the class stream; `B` is the fully charged procedural side under
//! each base strategy. The controls (random cohorts, best-member prototype,
//! `Literal`-only) are printed next to the candidate so a reader can see whether
//! a gain survives its falsifier.
//!
//! Both sides price an isolated class stream. That limitation is stated verbatim
//! in the rendered report because the number is otherwise easy to over-read.

use crate::archive;

use super::cohort::{self, Cohort, Rng};
use super::cost::{self, CohortReport, Eval, Mode};
use super::extract::{self, ClassName};
use super::{ClassSelector, Options};

/// One class's full result.
#[derive(Clone, Debug)]
pub struct ClassReport {
    pub class: &'static str,
    pub spans: usize,
    pub dropped: usize,
    pub stream_bytes: u64,
    /// A: `encode_tuned(S)` container bytes.
    pub a: u64,
    pub synth: Eval,
    pub random: Eval,
    pub best: Eval,
    pub literal: Eval,
    pub cohorts: Vec<CohortReport>,
    pub random_cohorts: Vec<CohortReport>,
}

impl ClassReport {
    /// `B - A` for the candidate, as a signed integer (negative is a win).
    pub fn delta_synth(&self) -> i64 {
        self.synth.total() as i64 - self.a as i64
    }
    pub fn delta_random(&self) -> i64 {
        self.random.total() as i64 - self.a as i64
    }
    pub fn delta_best(&self) -> i64 {
        self.best.total() as i64 - self.a as i64
    }
    pub fn delta_literal(&self) -> i64 {
        self.literal.total() as i64 - self.a as i64
    }
}

/// The whole experiment's result.
#[derive(Clone, Debug)]
pub struct Report {
    pub corpus: String,
    pub corpus_bytes: u64,
    /// The accepted configuration's archive size for the isolated corpus.
    pub corpus_archive_bytes: u64,
    pub classes: Vec<ClassReport>,
}

impl Report {
    /// The smallest `B - A` over every class and the candidate representation —
    /// the "best actual ΔS" P14.9 asks for.
    pub fn best_delta_synth(&self) -> Option<(i64, &ClassReport)> {
        self.classes
            .iter()
            .map(|c| (c.delta_synth(), c))
            .min_by_key(|(d, _)| *d)
    }

    /// Render the report.
    pub fn render(&self) -> String {
        let mut s = String::new();
        s.push_str("Phase 14.9 — first-boundary experiment (procedural vs accepted)\n");
        s.push_str("===============================================================\n\n");
        s.push_str(&format!(
            "corpus                      : {} ({} B)\n",
            self.corpus, self.corpus_bytes
        ));
        s.push_str(&format!(
            "S (corpus archive, accepted): {} B   [MEASURED, isolated corpus]\n",
            self.corpus_archive_bytes
        ));
        s.push_str("\nLIMITATION (verbatim): both sides are measured on an isolated class stream with no\ncross-span context, so the comparison is a NECESSARY condition for the procedural\nrepresentation, not a sufficient one. A win isolation-against-isolation is what P14.9\nrequires; a claim about in-pipeline cost is not.\n\n");

        s.push_str("Class-by-class (bytes; A = accepted coding of S):\n");
        s.push_str("  class      | spans |   |S|   |      A |  B synth |  B rndm |  B best | B litrl | dS synth\n");
        s.push_str("  -----------+-------+---------+--------+----------+---------+---------+---------+---------\n");
        for c in &self.classes {
            s.push_str(&format!(
                "  {:10} | {:5} | {:7} | {:6} | {:8} | {:7} | {:7} | {:7} | {:>8}\n",
                c.class,
                c.spans,
                c.stream_bytes,
                c.a,
                c.synth.total(),
                c.random.total(),
                c.best.total(),
                c.literal.total(),
                c.delta_synth()
            ));
        }

        for c in &self.classes {
            s.push_str(&format!(
                "\n--- class {} (spans={}, dropped={}, |S|={} B) ---\n",
                c.class, c.spans, c.dropped, c.stream_bytes
            ));
            s.push_str(&format!(
                "A  accepted coding of S                 : {} B  [MEASURED]\n",
                c.a
            ));
            s.push_str(&format!(
                "B  synthesised skeleton                 : {} B  (program {} + state {} [{}] + residual {} + decoder {} UNMEASURED + sep {})  dS {}\n",
                c.synth.total(), c.synth.program, c.synth.state, c.synth.state_codec,
                c.synth.residual, c.synth.decoder, c.synth.separators, c.delta_synth()
            ));
            s.push_str(&format!(
                "B  random cohorts (control)             : {} B  (program {} + state {} + residual {} + sep {})  dS {}\n",
                c.random.total(), c.random.program, c.random.state, c.random.residual,
                c.random.separators, c.delta_random()
            ));
            s.push_str(&format!(
                "B  best-member prototype (control)      : {} B  (program {} + state {} + residual {} + sep {})  dS {}\n",
                c.best.total(), c.best.program, c.best.state, c.best.residual,
                c.best.separators, c.delta_best()
            ));
            s.push_str(&format!(
                "B  Literal-only, no sharing (control)   : {} B  (program {} + state {} + residual {} + sep {})  dS {}\n",
                c.literal.total(), c.literal.program, c.literal.state, c.literal.residual,
                c.literal.separators, c.delta_literal()
            ));
            s.push_str("  per-cohort (real cohorts, synthesised skeleton):\n");
            for coh in top_cohorts(&c.cohorts) {
                s.push_str(&format!(
                    "    id={:016x} members={:5} raw={:7} B prog={:5} state={:5} resid_raw={:7}\n",
                    coh.id, coh.members, coh.member_bytes, coh.program, coh.state, coh.residual_raw
                ));
            }
            if c.cohorts.len() > TOP_COHORTS {
                s.push_str(&format!(
                    "    ... {} more cohorts (of {} total)\n",
                    c.cohorts.len() - TOP_COHORTS,
                    c.cohorts.len()
                ));
            }
            s.push_str("  negative control at equal cohort-size distribution (random):\n");
            for coh in top_cohorts(&c.random_cohorts) {
                s.push_str(&format!(
                    "    id={:016x} members={:5} resid_raw={:7}\n",
                    coh.id, coh.members, coh.residual_raw
                ));
            }
            if c.random_cohorts.len() > TOP_COHORTS {
                s.push_str(&format!(
                    "    ... {} more cohorts (of {} total)\n",
                    c.random_cohorts.len() - TOP_COHORTS,
                    c.random_cohorts.len()
                ));
            }
        }

        s.push_str("\nVerdict inputs:\n");
        match self.best_delta_synth() {
            Some((d, c)) => {
                s.push_str(&format!(
                    "  best actual dS (synthesised skeleton): {} B on class {} [MEASURED]\n",
                    d, c.class
                ));
                if d < 0 {
                    s.push_str(&format!(
                        "  -> a win in isolation; remaining headroom vs A: {} B [MEASURED]\n",
                        -d
                    ));
                } else {
                    s.push_str(&format!(
                        "  -> NO win in isolation; the residual must shrink by {} B to tie A [MEASURED]\n",
                        d
                    ));
                }
            }
            None => s.push_str("  no classes measured\n"),
        }
        s.push_str(
            "  decoder bytes are reported as 0 and labelled UNMEASURED; they are not silently omitted\n",
        );
        s.push_str(
            "  every B >= A result is a negative result for the procedural family on that class, and is\n  reported as such rather than hidden.\n",
        );
        s
    }
}

const TOP_COHORTS: usize = 8;

fn top_cohorts(cohorts: &[CohortReport]) -> Vec<CohortReport> {
    let mut v: Vec<CohortReport> = cohorts.to_vec();
    // Biggest cohorts first, then by id, so the report is deterministic.
    v.sort_by(|a, b| {
        b.member_bytes
            .cmp(&a.member_bytes)
            .then(a.id.cmp(&b.id))
            .then(a.members.cmp(&b.members))
    });
    v.truncate(TOP_COHORTS);
    v
}

/// Run the experiment and build the report.
pub fn analyse(path: &str, data: &[u8], opts: &Options) -> Result<Report, String> {
    let classes: Vec<ClassName> = match opts.class {
        ClassSelector::One(c) => vec![c],
        ClassSelector::All => ClassName::ALL.to_vec(),
    };
    let corpus_archive_bytes = if opts.corpus_s && !data.is_empty() {
        archive::encode(data).len() as u64
    } else {
        0
    };

    let mut out = Vec::new();
    for class in classes {
        let ex = extract::extract(data, class, opts.limit, opts.min_span, opts.max_span);
        let stream = cost::class_stream(data, &ex.spans);
        let a = cost::cost_coded(&stream);
        let cohorts: Vec<Cohort> = cohort::discover(data, &ex.spans, class);

        let (synth, cohort_detail) = cost::evaluate(data, &ex.spans, &cohorts, Mode::Synth, opts.min_cohort)?;

        let (random, random_detail) = if opts.random_cohorts && !cohorts.is_empty() {
            let mut rng = Rng::new(opts.seed);
            let rc = cohort::randomised(&cohorts, &mut rng);
            cost::evaluate(data, &ex.spans, &rc, Mode::Synth, opts.min_cohort)?
        } else {
            (Eval::default(), Vec::new())
        };

        let (best, _) = if opts.control_best_member {
            cost::evaluate(data, &ex.spans, &cohorts, Mode::BestMember, opts.min_cohort)?
        } else {
            (Eval::default(), Vec::new())
        };

        let (literal, _) = cost::evaluate(data, &ex.spans, &cohorts, Mode::LiteralOnly, opts.min_cohort)?;

        out.push(ClassReport {
            class: class.name(),
            spans: ex.spans.len(),
            dropped: ex.dropped,
            stream_bytes: stream.len() as u64,
            a,
            synth,
            random,
            best,
            literal,
            cohorts: cohort_detail,
            random_cohorts: random_detail,
        });
    }

    Ok(Report {
        corpus: path.to_string(),
        corpus_bytes: data.len() as u64,
        corpus_archive_bytes,
        classes: out,
    })
}
