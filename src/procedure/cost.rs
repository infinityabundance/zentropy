//! Cost accounting for the two sides of the boundary experiment.
//!
//! Both sides are measured **on the same bytes, through the project's own
//! accepted coder**:
//!
//! * **A** — the class stream `S`: the raw concatenation of the selected spans
//!   with an explicit `\n` separator between them. `A = len(encode_tuned(S))`,
//!   the real container bytes.
//! * **B** — for each cohort, `serialize(program).len()` of the shared skeleton +
//!   the per-member state bytes + the cost of coding the length-framed
//!   concatenated residual stream through the *same* accepted configuration +
//!   `decoder` (marginal decoder bytes; reported as 0 and labelled unmeasured,
//!   because the driver's decoder does not yet carry this representation) + the
//!   same separator bytes charged to A.
//!
//! The separator framing is charged to **both** sides. A needs it to delimit
//! spans; B does not, but charging it to B as well keeps the comparison from
//! flattering the procedural side.
//!
//! ## The stated limitation
//!
//! Both sides price an **isolated class stream with no cross-span context**. A
//! real submission codes the class stream in place, amid all the other structure
//! the mixer can condition on, so `B < A` here is a *necessary* condition for the
//! procedural representation, never a sufficient one. This experiment answers the
//! narrow question P14.9 asks — does the representation have any headroom at all
//! in isolation — and nothing about in-pipeline cost.

use crate::archive::{self, ACCEPTED_METHOD, ACCEPTED_TUNE};

use super::cohort::Cohort;
use super::extract::Span;
use super::{member, skeleton};

/// Which shared-base strategy to cost.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// Synthesised anti-unifier skeleton (the candidate).
    Synth,
    /// The cohort's best actual member as the shared base (§14.44 prototype
    /// control).
    BestMember,
    /// One `Literal` program per member, nothing shared (the `Literal`-only
    /// control).
    LiteralOnly,
}

/// A fully charged breakdown of one side. Every field is measured bytes.
#[derive(Clone, Debug, Default)]
pub struct Eval {
    /// Sum of `serialize(program).len()` over shared programs.
    pub program: u64,
    /// Per-member state bytes (hole lengths through the state codec).
    pub state: u64,
    /// Coded bytes of the concatenated, length-framed residual stream.
    pub residual: u64,
    /// Marginal decoder bytes. **Unmeasured**; carried as 0 and labelled.
    pub decoder: u64,
    /// Separator bytes, charged to both sides.
    pub separators: u64,
    /// Which state codec won, for the receipt.
    pub state_codec: String,
}

impl Eval {
    pub fn total(&self) -> u64 {
        self.program
            .saturating_add(self.state)
            .saturating_add(self.residual)
            .saturating_add(self.decoder)
            .saturating_add(self.separators)
    }
}

/// Per-cohort detail, so a win or loss can be attributed to a cohort rather than
/// asserted in aggregate.
#[derive(Clone, Debug)]
pub struct CohortReport {
    pub id: u64,
    pub members: usize,
    /// Raw bytes of the cohort's members before coding.
    pub member_bytes: u64,
    /// Shared program bytes.
    pub program: u64,
    /// State bytes for this cohort.
    pub state: u64,
    /// Raw (uncoded) residual wire bytes for this cohort.
    pub residual_raw: u64,
}

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

/// `S`: the selected spans concatenated with an explicit separator byte.
pub fn class_stream(data: &[u8], spans: &[Span]) -> Vec<u8> {
    let mut out = Vec::new();
    for (i, s) in spans.iter().enumerate() {
        if i > 0 {
            out.push(b'\n');
        }
        out.extend_from_slice(&data[s.start..s.end()]);
    }
    out
}

/// The accepted configuration's real container length for a stream. Empty input
/// costs zero (an empty stream is not coded at all).
pub fn cost_coded(stream: &[u8]) -> u64 {
    if stream.is_empty() {
        0
    } else {
        archive::encode_tuned(stream, ACCEPTED_METHOD, ACCEPTED_TUNE).len() as u64
    }
}

/// Cost one representation of the spans under `mode`.
pub fn evaluate(
    data: &[u8],
    spans: &[Span],
    cohorts: &[Cohort],
    mode: Mode,
    min_cohort: usize,
) -> Result<(Eval, Vec<CohortReport>), String> {
    let sep = spans.len().saturating_sub(1) as u64;
    let mut ev = Eval {
        separators: sep,
        ..Default::default()
    };
    let mut reports: Vec<CohortReport> = Vec::new();
    if spans.is_empty() {
        return Ok((ev, reports));
    }

    if mode == Mode::LiteralOnly {
        let mut program = 0u64;
        for s in spans {
            let sk = skeleton::literal(&data[s.start..s.end()]);
            program += sk.program_bytes as u64;
            reports.push(CohortReport {
                id: 0,
                members: 1,
                member_bytes: s.len as u64,
                program: sk.program_bytes as u64,
                state: 0,
                residual_raw: 0,
            });
        }
        ev.program = program;
        return Ok((ev, reports));
    }

    let mut residual_stream: Vec<u8> = Vec::new();
    let mut program_total = 0u64;
    let mut state_total = 0u64;

    for c in cohorts {
        let members: Vec<Vec<u8>> = c
            .members
            .iter()
            .map(|&i| data[spans[i].start..spans[i].end()].to_vec())
            .collect();
        if members.is_empty() {
            continue;
        }
        let member_bytes: u64 = members.iter().map(|m| m.len() as u64).sum();

        let (sk, base, hole_lens): (skeleton::Skeleton, Vec<u8>, Option<Vec<usize>>) = match mode {
            Mode::Synth => {
                let sk = skeleton::synthesise_with_min(&members, min_cohort);
                let base = skeleton::materialise_base(&sk)?;
                if base != sk.base() {
                    return Err("procedure: synthesised base differs from prefix ++ suffix".into());
                }
                let pl = sk.prefix.len();
                let sl = sk.suffix.len();
                let mut lens = Vec::with_capacity(members.len());
                for m in &members {
                    if pl + sl > m.len() {
                        return Err("procedure: skeleton prefix/suffix exceeds a member".into());
                    }
                    let hole = &m[pl..m.len() - sl];
                    // The program must reproduce the member from its own hole.
                    let got = skeleton::materialise_with_hole(&sk, hole)?;
                    if got.as_slice() != m.as_slice() {
                        return Err("procedure: skeleton does not reproduce a member".into());
                    }
                    lens.push(hole.len());
                }
                (sk, base, Some(lens))
            }
            Mode::BestMember => {
                let base = member::best_member(&members);
                let sk = skeleton::literal(&base);
                if skeleton::materialise_base(&sk)? != base {
                    return Err("procedure: literal base does not materialise".into());
                }
                (sk, base, None)
            }
            Mode::LiteralOnly => unreachable!(),
        };

        program_total += sk.program_bytes as u64;
        let mut residual_raw = 0u64;
        for m in &members {
            let r = member::best_residual(&base, m);
            if !member::verify(&base, m, &r) {
                return Err("procedure: residual round-trip failed for a member".into());
            }
            let w = member::wire(&r);
            residual_raw += w.len() as u64;
            put_uvarint(&mut residual_stream, w.len() as u64);
            residual_stream.extend_from_slice(&w);
        }

        let (state_bytes, codec) = match &hole_lens {
            Some(lens) => member::encode_lengths(lens)?,
            None => (0usize, "none".to_string()),
        };
        if !codec.is_empty() && codec != "none" {
            ev.state_codec = codec;
        }
        state_total += state_bytes as u64;

        reports.push(CohortReport {
            id: c.id,
            members: members.len(),
            member_bytes,
            program: sk.program_bytes as u64,
            state: state_bytes as u64,
            residual_raw,
        });
    }

    ev.program = program_total;
    ev.state = state_total;
    ev.residual = cost_coded(&residual_stream);
    Ok((ev, reports))
}

#[cfg(test)]
mod tests {
    use super::super::cohort;
    use super::super::extract::{self, ClassName};
    use super::*;

    fn spans_of(data: &[u8]) -> Vec<Span> {
        extract::extract(data, ClassName::Template, 1000, 1, 1 << 20).spans
    }

    #[test]
    fn the_separators_are_charged_to_both_sides() {
        let data = b"{{a|1}} {{a|2}} {{a|3}}";
        let spans = spans_of(data);
        let cohorts = cohort::discover(data, &spans, ClassName::Template);
        let (ev, _) = evaluate(data, &spans, &cohorts, Mode::Synth, cohort::DEFAULT_MIN_COHORT).unwrap();
        assert_eq!(ev.separators, (spans.len() - 1) as u64);
        assert!(ev.total() >= ev.separators);
    }

    #[test]
    fn the_total_includes_the_coded_residual_stream() {
        // This is the test that fails if the residual stream were not charged: the
        // synthesised skeleton's shared parts cannot reconstruct the members, so
        // the coded residual must be strictly positive.
        let data = b"{{cite|a=1}}{{cite|b=2}}{{cite|c=3}}";
        let spans = spans_of(data);
        let cohorts = cohort::discover(data, &spans, ClassName::Template);
        let (ev, reps) = evaluate(data, &spans, &cohorts, Mode::Synth, cohort::DEFAULT_MIN_COHORT).unwrap();
        assert!(ev.residual > 0, "residual stream was not charged");
        let raw: u64 = reps.iter().map(|r| r.residual_raw).sum();
        assert!(raw > 0);
        // Dropping the residual component from the total changes it.
        let without = ev.program + ev.state + ev.decoder + ev.separators;
        assert!(ev.total() > without);
        assert_eq!(ev.total() - without, ev.residual);
    }

    #[test]
    fn literal_only_charges_one_program_per_member_and_no_residual() {
        let data = b"{{a|1}} {{b|2}} {{c|3}}";
        let spans = spans_of(data);
        let cohorts = cohort::discover(data, &spans, ClassName::Template);
        let (ev, reps) = evaluate(data, &spans, &cohorts, Mode::LiteralOnly, cohort::DEFAULT_MIN_COHORT).unwrap();
        assert_eq!(reps.len(), spans.len());
        assert_eq!(ev.residual, 0);
        let sum_bytes: u64 = spans.iter().map(|s| s.len as u64).sum();
        assert!(
            ev.program >= sum_bytes,
            "literal programs must carry the bytes: program={} bytes={}",
            ev.program,
            sum_bytes
        );
    }

    #[test]
    fn the_class_stream_is_the_spans_joined_by_separators() {
        let data = b"{{a|1}}XX{{b|2}}";
        let spans = spans_of(data);
        let s = class_stream(data, &spans);
        assert_eq!(s, b"{{a|1}}\n{{b|2}}".to_vec());
    }

    #[test]
    fn best_member_control_round_trips_every_member() {
        let data = b"{{cite|url=1|t=x}}{{cite|url=2|t=y}}{{cite|url=3|t=z}}";
        let spans = spans_of(data);
        let cohorts = cohort::discover(data, &spans, ClassName::Template);
        let (ev, _) = evaluate(data, &spans, &cohorts, Mode::BestMember, cohort::DEFAULT_MIN_COHORT).unwrap();
        assert!(ev.residual > 0);
        assert!(ev.total() >= ev.program + ev.residual);
    }
}
