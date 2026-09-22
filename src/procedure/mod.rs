//! Phase 14.9 — the first-boundary experiment.
//!
//! **The question.** Is there a real Wikipedia-derived class where
//!
//! ```text
//! program + state + residual + marginal decoder cost  <  the accepted representation
//! ```
//!
//! over the *same bytes*? This module answers it with a measurement **and its
//! falsifying control**, not with a projection.
//!
//! ## What it builds
//!
//! 1. **Class extraction** — the spans of one IR [`crate::ir::Kind`] (`template`,
//!    `wiki_link`, `wiki_table`, `xml_open`, `number`, `url`, `entity`) as
//!    *targets*, bounded by a count and a size window. Malformed or overlapping
//!    spans are dropped, never repaired.
//! 2. **Cohort discovery** (§14.13) — a deterministic partition by cheap
//!    corpus-native signatures: template *names*, xml *tag names*, or the IR
//!    *shape*.
//! 3. **Skeleton synthesis** (§14.14) — one shared anti-unifier per cohort: the
//!    cohort's byte-level common prefix/suffix with the differing interior region
//!    as a typed slot. It is emitted as a real [`crate::procedural`] `Program` and
//!    charged at `serialize(program).len()`. Every extracted target is a single IR
//!    token, so the per-span `Kind` sequence has length one and there is no finer
//!    kind structure to align to; the `Kind` sequence serves as the cohort
//!    signature for classes without a name signature.
//! 4. **State + residual** — the slot geometry as a bounded-count state coded by
//!    the best `procedural::state` codec, and the innovation as the smallest
//!    typed residual, with `apply(base, derive(base, target)) == target` verified
//!    for every member.
//! 5. **Cost accounting on the same bytes** — A is the accepted coder's real
//!    container bytes for the class stream `S`; B is program + state + coded
//!    residual + decoder + the same separator bytes.
//!
//! ## Controls
//!
//! * `--random-cohorts` — identical cohort-size distribution, random assignment.
//! * `--control-best-member` — the cohort medoid as the shared base instead of a
//!   synthesised skeleton.
//! * `Literal`-only — one verbatim program per member, nothing shared.
//!
//! ## The honest limitation
//!
//! Both sides price an **isolated** class stream with no cross-span context, so a
//! `B < A` result is a *necessary* condition for the procedural representation,
//! not a sufficient one. The report prints this verbatim.

use std::fs;

mod cohort;
mod cost;
mod extract;
mod member;
mod report;
mod skeleton;

pub use extract::ClassName;
pub use report::Report;

/// Which classes to run: one, or every named class in turn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClassSelector {
    One(ClassName),
    All,
}

/// Experiment configuration, parsed from the driver's arguments.
#[derive(Clone, Debug)]
pub struct Options {
    pub class: ClassSelector,
    /// Maximum number of targets kept per class.
    pub limit: usize,
    pub min_span: usize,
    pub max_span: usize,
    /// Run the random-cohort control (mandatory by policy; on by default).
    pub random_cohorts: bool,
    /// Run the best-member-prototype control.
    pub control_best_member: bool,
    /// Seed for the random-cohort control.
    pub seed: u64,
    /// Also measure the accepted archive of the isolated corpus (the "current S").
    pub corpus_s: bool,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            class: ClassSelector::One(ClassName::Template),
            limit: 5000,
            min_span: 4,
            max_span: 8192,
            random_cohorts: true,
            control_best_member: true,
            seed: 0x9e37_79b9_7f4a_7c15,
            corpus_s: true,
        }
    }
}

/// Parse `--class`, `--limit`, `--min-span`, `--max-span`, `--seed`, and the
/// control switches. `--class all` runs every named class.
pub fn parse_args(args: &[String]) -> Result<Options, String> {
    let mut o = Options::default();
    let mut i = 0usize;
    while i < args.len() {
        let a = &args[i];
        let mut next = |what: &str| -> Result<String, String> {
            i += 1;
            args.get(i)
                .cloned()
                .ok_or_else(|| format!("procedure: {what} needs a value"))
        };
        match a.as_str() {
            "--class" => {
                let v = next("--class")?;
                o.class = if v == "all" {
                    ClassSelector::All
                } else {
                    ClassSelector::One(
                        ClassName::from_name(&v)
                            .ok_or_else(|| format!("procedure: unknown class '{v}'"))?,
                    )
                };
            }
            "--limit" => {
                let v = next("--limit")?;
                o.limit = v
                    .parse()
                    .map_err(|_| format!("procedure: bad --limit '{v}'"))?;
            }
            "--min-span" => {
                let v = next("--min-span")?;
                o.min_span = v
                    .parse()
                    .map_err(|_| format!("procedure: bad --min-span '{v}'"))?;
            }
            "--max-span" => {
                let v = next("--max-span")?;
                o.max_span = v
                    .parse()
                    .map_err(|_| format!("procedure: bad --max-span '{v}'"))?;
            }
            "--seed" => {
                let v = next("--seed")?;
                o.seed = v
                    .parse()
                    .map_err(|_| format!("procedure: bad --seed '{v}'"))?;
            }
            "--random-cohorts" => o.random_cohorts = true,
            "--no-random-cohorts" => o.random_cohorts = false,
            "--control-best-member" => o.control_best_member = true,
            "--no-control-best-member" => o.control_best_member = false,
            "--no-corpus-s" => o.corpus_s = false,
            other => return Err(format!("procedure: unknown option '{other}'")),
        }
        i += 1;
    }
    if o.min_span == 0 || o.max_span < o.min_span {
        return Err("procedure: require 1 <= min-span <= max-span".into());
    }
    Ok(o)
}

/// Run the experiment. The driver calls
/// `zentropy::procedure::run(path, &args[1..])` and prints `report.render()`.
pub fn run(path: &str, args: &[String]) -> Result<Report, String> {
    let opts = parse_args(args)?;
    let data = fs::read(path).map_err(|e| format!("procedure: cannot read {path}: {e}"))?;
    report::analyse(path, &data, &opts)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_documented_flags() {
        let args: Vec<String> = vec![
            "--class",
            "xml_open",
            "--limit",
            "17",
            "--min-span",
            "2",
            "--max-span",
            "64",
            "--seed",
            "5",
            "--no-corpus-s",
        ]
        .into_iter()
        .map(String::from)
        .collect();
        let o = parse_args(&args).unwrap();
        assert_eq!(o.class, ClassSelector::One(ClassName::XmlOpen));
        assert_eq!(o.limit, 17);
        assert_eq!(o.min_span, 2);
        assert_eq!(o.max_span, 64);
        assert_eq!(o.seed, 5);
        assert!(!o.corpus_s);
        assert!(o.random_cohorts && o.control_best_member);
    }

    #[test]
    fn class_all_selects_every_class() {
        let args: Vec<String> = vec!["--class".into(), "all".into()];
        assert_eq!(parse_args(&args).unwrap().class, ClassSelector::All);
    }

    #[test]
    fn rejects_unknown_class_and_bad_window() {
        assert!(parse_args(&[String::from("--class"), String::from("nope")]).is_err());
        assert!(parse_args(&[
            String::from("--min-span"),
            String::from("9"),
            String::from("--max-span"),
            String::from("3"),
        ])
        .is_err());
    }

    /// End-to-end on the development rung. Run deliberately:
    ///
    /// ```text
    /// tools/memcap.sh 10 cargo test --release --lib --features procedural,opportunity \
    ///     -- --ignored --nocapture procedure::tests::enwik6_e2e_prints_report
    /// ```
    #[test]
    #[ignore = "end-to-end on evidence/corpus/enwik6; run deliberately"]
    fn enwik6_e2e_prints_report() {
        // Defaults reproduce the documented command; the env overrides exist so the
        // same deliberate test can produce the class-by-class table without a driver
        // arm being wired yet.
        let class = std::env::var("PROCEDURE_CLASS").unwrap_or_else(|_| "template".to_string());
        let limit = std::env::var("PROCEDURE_LIMIT").unwrap_or_else(|_| "2000".to_string());
        let args: Vec<String> = vec!["--class", class.as_str(), "--limit", limit.as_str()]
            .into_iter()
            .map(String::from)
            .collect();
        let report = run("evidence/corpus/enwik6", &args).expect("procedure run");
        print!("{}", report.render());
        assert!(!report.classes.is_empty());
    }
}
