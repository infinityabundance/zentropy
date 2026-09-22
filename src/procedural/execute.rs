//! Materialize a [`Program`] exactly, or fail with a typed error.
//!
//! **The contract.** `execute` either returns the program's full output or an
//! [`ExecError`]; it never returns a partial materialization, never panics, never
//! OOMs and never hangs. Those are not aspirations, they are the four ways a
//! forged archive could take down a judged decoder, so each has a mechanism:
//!
//! * *partial* — nothing outside `execute` is mutated until success;
//! * *panic* — every index is checked, every add/multiply is checked, and the
//!   recursion depth is capped by `max_depth` before it can exhaust the stack;
//! * *OOM* — `max_output` is checked against the *computed* length of an output
//!   before the buffer is allocated, so the classic `Repeat` bomb (a long child
//!   times a large count) is refused rather than reserved;
//! * *hang* — references are followed with cycle detection, so a self-referential
//!   program terminates with [`ExecError::RefCycle`] instead of recursing forever.
//!
//! **Five independent ceilings.** Depth, node count, output length, work and
//! reference count are each enforced separately and each has its own variant, so
//! one shadowed check cannot quietly excuse the others (plan §0.5, §4.2). A
//! program that resolves more external material references (`RefTarget::Material`)
//! than `max_refs` permits is reading outside the material the decoder sanctions,
//! so the reference-count ceiling is reported as [`ExecError::BadRef`].
//!
//! **Why memoize.** A shared program is a DAG; a node referenced twice must be
//! materialized once, or sharing would be a cost with no benefit. Memoization
//! also makes cycle detection a single "in progress" flag per node.

use super::program::{Op, Program};
use super::residual::{self, Residual};
use super::types::{NodeId, RefTarget};
use super::{Bounds, ExecError};

/// Everything a program needs that is not itself part of the program: material
/// the caller already decoded and the residual pool.
///
/// Kept separate from [`Program`] on purpose. `serialize(program)` is the charged
/// program cost; external material and residuals are charged in their own streams
/// (plan §4.4), so folding them into the program would double-count — or hide —
/// their bytes.
#[derive(Debug, Default)]
pub struct ExecContext<'a> {
    /// Already-materialised byte strings, indexed by [`RefTarget::Material`].
    pub refs: Vec<&'a [u8]>,
    /// Residuals applied by [`Op::Patch`], indexed by [`super::types::ResidualId`].
    pub residuals: Vec<Residual>,
}

impl<'a> ExecContext<'a> {
    /// An empty context: no external material, no residuals.
    pub fn new() -> Self {
        ExecContext {
            refs: Vec::new(),
            residuals: Vec::new(),
        }
    }
}

/// Materialize `p` under `bounds`, or fail typed. Never partially.
pub fn execute(p: &Program, cx: &ExecContext, bounds: &Bounds) -> Result<Vec<u8>, ExecError> {
    // Node count is the cheapest bound to check and the one that sizes the caches,
    // so it is checked before allocating anything.
    if p.nodes.len() as u64 > bounds.max_nodes as u64 {
        return Err(ExecError::Nodes);
    }
    if p.root.0 as usize >= p.nodes.len() {
        return Err(ExecError::BadRef);
    }
    let mut vm = Vm {
        memo: vec![None; p.nodes.len()],
        active: vec![false; p.nodes.len()],
        work: 0,
        refs_used: 0,
    };
    vm.eval(p, cx, bounds, p.root, 0, None)
}

struct Vm {
    /// Materialized output per node, once computed.
    memo: Vec<Option<Vec<u8>>>,
    /// Nodes currently on the evaluation stack, for cycle detection.
    active: Vec<bool>,
    /// Work charged so far (node evaluations plus bytes produced).
    work: u64,
    /// External material references resolved so far.
    refs_used: u64,
}

impl Vm {
    fn charge(&mut self, k: u64, b: &Bounds) -> Result<(), ExecError> {
        self.work = self.work.checked_add(k).ok_or(ExecError::Work)?;
        if self.work > b.max_work {
            return Err(ExecError::Work);
        }
        Ok(())
    }

    /// Evaluate node `id`. `slots` is the current template's slot table, if any;
    /// a nested template replaces it entirely (slots are not inherited).
    fn eval(
        &mut self,
        p: &Program,
        cx: &ExecContext,
        b: &Bounds,
        id: NodeId,
        depth: u32,
        slots: Option<&[Vec<u8>]>,
    ) -> Result<Vec<u8>, ExecError> {
        if depth > b.max_depth as u32 {
            return Err(ExecError::Depth);
        }
        let idx = id.0 as usize;
        if idx >= p.nodes.len() {
            return Err(ExecError::BadRef);
        }
        if self.active[idx] {
            return Err(ExecError::RefCycle);
        }
        if let Some(m) = self.memo[idx].as_ref() {
            return Ok(m.clone());
        }
        self.active[idx] = true;
        self.charge(1, b)?;

        let node = &p.nodes[idx];
        let out = match &node.op {
            Op::Literal(l) => {
                let lit = p.literals.get(l.0 as usize).ok_or(ExecError::BadRef)?;
                if lit.len() as u64 > b.max_output {
                    return Err(ExecError::Output);
                }
                lit.clone()
            }
            Op::Concat { parts } => {
                let mut out = self.eval(p, cx, b, parts[0], depth + 1, slots)?;
                let second = self.eval(p, cx, b, parts[1], depth + 1, slots)?;
                let total = out
                    .len()
                    .checked_add(second.len())
                    .ok_or(ExecError::Output)?;
                if total as u64 > b.max_output {
                    return Err(ExecError::Output);
                }
                out.extend_from_slice(&second);
                out
            }
            Op::Repeat { child, count } => {
                let child_bytes = self.eval(p, cx, b, *child, depth + 1, slots)?;
                // The allocation bomb: child length times count. Compute in u128 so
                // the product itself cannot wrap, and refuse *before* allocating.
                let total = (child_bytes.len() as u128) * (*count as u128);
                if total > b.max_output as u128 {
                    return Err(ExecError::Output);
                }
                let total = total as u64;
                if total > usize::MAX as u64 {
                    return Err(ExecError::Output);
                }
                if child_bytes.is_empty() {
                    // Avoid a loop that would iterate `count` times doing nothing.
                    if self.work.checked_add(1).map_or(true, |w| w > b.max_work) {
                        return Err(ExecError::Work);
                    }
                    Vec::new()
                } else {
                    if self
                        .work
                        .checked_add(total)
                        .map_or(true, |w| w > b.max_work)
                    {
                        return Err(ExecError::Work);
                    }
                    let mut out = Vec::with_capacity(total as usize);
                    for _ in 0..*count {
                        out.extend_from_slice(&child_bytes);
                    }
                    out
                }
            }
            Op::Ref { target } => match target {
                RefTarget::Node(n) => self.eval(p, cx, b, *n, depth + 1, slots)?,
                RefTarget::Material(i) => {
                    self.refs_used = self.refs_used.checked_add(1).ok_or(ExecError::BadRef)?;
                    if self.refs_used > b.max_refs as u64 {
                        return Err(ExecError::BadRef);
                    }
                    let m = cx.refs.get(*i as usize).ok_or(ExecError::BadRef)?;
                    if m.len() as u64 > b.max_output {
                        return Err(ExecError::Output);
                    }
                    m.to_vec()
                }
                RefTarget::Slot(i) => {
                    // A slot reference with no active template, or one past the
                    // current template's slots, names material that does not exist.
                    let v = slots
                        .and_then(|s| s.get(*i as usize))
                        .ok_or(ExecError::BadRef)?;
                    if v.len() as u64 > b.max_output {
                        return Err(ExecError::Output);
                    }
                    v.clone()
                }
            },
            Op::Slice { src, from, len } => {
                let src_bytes = self.eval(p, cx, b, *src, depth + 1, slots)?;
                let from = *from as usize;
                let len = *len as usize;
                let end = from.checked_add(len).ok_or(ExecError::BadRef)?;
                if end > src_bytes.len() {
                    return Err(ExecError::BadRef);
                }
                if len as u64 > b.max_output {
                    return Err(ExecError::Output);
                }
                src_bytes[from..end].to_vec()
            }
            Op::Template {
                program,
                slots: slot_nodes,
            } => {
                let mut values: Vec<Vec<u8>> = Vec::with_capacity(slot_nodes.len());
                for s in slot_nodes {
                    values.push(self.eval(p, cx, b, *s, depth + 1, slots)?);
                }
                self.eval(p, cx, b, *program, depth + 1, Some(&values))?
            }
            Op::Patch { base, residual } => {
                let base_bytes = self.eval(p, cx, b, *base, depth + 1, slots)?;
                let r = cx
                    .residuals
                    .get(residual.0 as usize)
                    .ok_or(ExecError::BadRef)?;
                match residual::apply(&base_bytes, r) {
                    Some(o) => {
                        if o.len() as u64 > b.max_output {
                            return Err(ExecError::Output);
                        }
                        o
                    }
                    // A rank that is not a legal coordinate is its own rejection;
                    // any other mismatch means the residual does not fit the base.
                    None => {
                        return Err(if matches!(r, Residual::RankedMismatch { .. }) {
                            ExecError::BadRank
                        } else {
                            ExecError::Malformed
                        })
                    }
                }
            }
        };

        // Defensive: an operator that forgot its own check must not slip a
        // too-large output past the bound.
        if out.len() as u64 > b.max_output {
            return Err(ExecError::Output);
        }
        self.charge(out.len() as u64, b)?;
        self.memo[idx] = Some(out.clone());
        self.active[idx] = false;
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::procedural::program::{Node, Op, Program};
    use crate::procedural::residual::{self, Residual, ResidualKind};
    use crate::procedural::types::{LiteralId, NodeId, RefTarget, ResidualId, Span};

    fn n(op: Op) -> Node {
        Node {
            op,
            span: Span::EMPTY,
        }
    }

    fn bounds() -> Bounds {
        Bounds {
            max_depth: 32,
            max_nodes: 64,
            max_output: 1 << 16,
            max_work: 1 << 20,
            max_refs: 16,
        }
    }

    fn run(p: &Program, cx: &ExecContext) -> Result<Vec<u8>, ExecError> {
        execute(p, cx, &bounds())
    }

    #[test]
    fn literal_executes_to_its_bytes() {
        let p = Program {
            nodes: vec![n(Op::Literal(LiteralId(0)))],
            literals: vec![b"hello".to_vec()],
            root: NodeId(0),
        };
        assert_eq!(run(&p, &ExecContext::new()).unwrap(), b"hello");
    }

    #[test]
    fn concat_is_ordered_left_then_right() {
        let p = Program {
            nodes: vec![
                n(Op::Literal(LiteralId(0))),
                n(Op::Literal(LiteralId(1))),
                n(Op::Concat {
                    parts: [NodeId(0), NodeId(1)],
                }),
            ],
            literals: vec![b"ab".to_vec(), b"cd".to_vec()],
            root: NodeId(2),
        };
        assert_eq!(run(&p, &ExecContext::new()).unwrap(), b"abcd");
    }

    #[test]
    fn repeat_multiplies_the_child() {
        let p = Program {
            nodes: vec![
                n(Op::Literal(LiteralId(0))),
                n(Op::Repeat {
                    child: NodeId(0),
                    count: 3,
                }),
            ],
            literals: vec![b"ab".to_vec()],
            root: NodeId(1),
        };
        assert_eq!(run(&p, &ExecContext::new()).unwrap(), b"ababab");
    }

    #[test]
    fn a_reference_shares_a_node_output_once() {
        // Two references to the same node must not be a cycle: the node is done
        // after the first evaluation, so the second read is a memo hit.
        let p = Program {
            nodes: vec![
                n(Op::Literal(LiteralId(0))),
                n(Op::Ref {
                    target: RefTarget::Node(NodeId(0)),
                }),
                n(Op::Ref {
                    target: RefTarget::Node(NodeId(0)),
                }),
                n(Op::Concat {
                    parts: [NodeId(1), NodeId(2)],
                }),
            ],
            literals: vec![b"x".to_vec()],
            root: NodeId(3),
        };
        assert_eq!(run(&p, &ExecContext::new()).unwrap(), b"xx");
    }

    #[test]
    fn a_reference_reads_caller_supplied_material() {
        let p = Program {
            nodes: vec![n(Op::Ref {
                target: RefTarget::Material(0),
            })],
            literals: vec![],
            root: NodeId(0),
        };
        let cx = ExecContext {
            refs: vec![b"material".as_slice()],
            residuals: vec![],
        };
        assert_eq!(run(&p, &cx).unwrap(), b"material");
    }

    #[test]
    fn slice_takes_the_requested_range() {
        let p = Program {
            nodes: vec![
                n(Op::Literal(LiteralId(0))),
                n(Op::Slice {
                    src: NodeId(0),
                    from: 2,
                    len: 3,
                }),
            ],
            literals: vec![b"abcdef".to_vec()],
            root: NodeId(1),
        };
        assert_eq!(run(&p, &ExecContext::new()).unwrap(), b"cde");
    }

    #[test]
    fn template_substitutes_its_slots_into_the_skeleton() {
        // Skeleton "<" + (slot + ">") materialized with slot 0 = "x": "<x>".
        let p = Program {
            nodes: vec![
                n(Op::Literal(LiteralId(0))), // 0: "x" (the slot value)
                n(Op::Literal(LiteralId(1))), // 1: "<"
                n(Op::Literal(LiteralId(2))), // 2: ">"
                n(Op::Ref {
                    target: RefTarget::Slot(0),
                }), // 3
                n(Op::Concat {
                    parts: [NodeId(3), NodeId(2)],
                }), // 4: "x>"
                n(Op::Concat {
                    parts: [NodeId(1), NodeId(4)],
                }), // 5: "<x>"
                n(Op::Template {
                    program: NodeId(5),
                    slots: vec![NodeId(0)],
                }), // 6
            ],
            literals: vec![b"x".to_vec(), b"<".to_vec(), b">".to_vec()],
            root: NodeId(6),
        };
        assert_eq!(run(&p, &ExecContext::new()).unwrap(), b"<x>");
    }

    #[test]
    fn patch_applies_a_residual_to_its_base() {
        let p = Program {
            nodes: vec![
                n(Op::Literal(LiteralId(0))),
                n(Op::Patch {
                    base: NodeId(0),
                    residual: ResidualId(0),
                }),
            ],
            literals: vec![b"abcdef".to_vec()],
            root: NodeId(1),
        };
        let r = residual::derive(b"abcdef", b"aXYdef", ResidualKind::RangeReplace).unwrap();
        let cx = ExecContext {
            refs: vec![],
            residuals: vec![r],
        };
        assert_eq!(run(&p, &cx).unwrap(), b"aXYdef");
    }

    #[test]
    fn bounds_reject_an_allocation_bomb() {
        // A small child times a large count. The product must be refused before the
        // buffer is reserved, and it must be the *output* bound that fires.
        let p = Program {
            nodes: vec![
                n(Op::Literal(LiteralId(0))),
                n(Op::Repeat {
                    child: NodeId(0),
                    count: 1_000_000,
                }),
            ],
            literals: vec![b"0123456789".to_vec()],
            root: NodeId(1),
        };
        let mut b = bounds();
        b.max_output = 1000;
        assert_eq!(execute(&p, &ExecContext::new(), &b), Err(ExecError::Output));
    }

    #[test]
    fn repeat_length_is_computed_without_wrapping() {
        // 100 * (2^32 - 1) wraps to a small value in 32-bit arithmetic. If the
        // product were computed at the wrong width the bomb would pass the bound.
        let p = Program {
            nodes: vec![
                n(Op::Literal(LiteralId(0))),
                n(Op::Repeat {
                    child: NodeId(0),
                    count: u32::MAX,
                }),
            ],
            literals: vec![vec![7u8; 100]],
            root: NodeId(1),
        };
        let mut b = bounds();
        b.max_output = 1 << 32;
        assert_eq!(execute(&p, &ExecContext::new(), &b), Err(ExecError::Output));
    }

    #[test]
    fn an_empty_repeat_returns_empty_without_iterating() {
        let p = Program {
            nodes: vec![
                n(Op::Literal(LiteralId(0))),
                n(Op::Repeat {
                    child: NodeId(0),
                    count: u32::MAX,
                }),
            ],
            literals: vec![Vec::new()],
            root: NodeId(1),
        };
        assert_eq!(run(&p, &ExecContext::new()).unwrap(), Vec::<u8>::new());
    }

    #[test]
    fn bounds_reject_nesting_past_max_depth() {
        let mut nodes: Vec<Node> = (0..6)
            .map(|i| {
                n(Op::Ref {
                    target: RefTarget::Node(NodeId(i + 1)),
                })
            })
            .collect();
        nodes.push(n(Op::Literal(LiteralId(0))));
        let p = Program {
            nodes,
            literals: vec![b"x".to_vec()],
            root: NodeId(0),
        };
        let mut b = bounds();
        b.max_depth = 2;
        assert_eq!(execute(&p, &ExecContext::new(), &b), Err(ExecError::Depth));
    }

    #[test]
    fn bounds_reject_more_nodes_than_permitted() {
        let p = Program {
            nodes: vec![
                n(Op::Literal(LiteralId(0))),
                n(Op::Literal(LiteralId(0))),
                n(Op::Concat {
                    parts: [NodeId(0), NodeId(1)],
                }),
            ],
            literals: vec![b"x".to_vec()],
            root: NodeId(2),
        };
        let mut b = bounds();
        b.max_nodes = 2;
        assert_eq!(execute(&p, &ExecContext::new(), &b), Err(ExecError::Nodes));
    }

    #[test]
    fn bounds_reject_more_work_than_permitted() {
        let p = Program {
            nodes: vec![n(Op::Literal(LiteralId(0)))],
            literals: vec![vec![0u8; 50]],
            root: NodeId(0),
        };
        let mut b = bounds();
        b.max_output = 100;
        b.max_work = 10;
        assert_eq!(execute(&p, &ExecContext::new(), &b), Err(ExecError::Work));
    }

    #[test]
    fn bounds_reject_more_external_references_than_permitted() {
        let p = Program {
            nodes: vec![
                n(Op::Ref {
                    target: RefTarget::Material(0),
                }),
                n(Op::Ref {
                    target: RefTarget::Material(1),
                }),
                n(Op::Concat {
                    parts: [NodeId(0), NodeId(1)],
                }),
            ],
            literals: vec![],
            root: NodeId(2),
        };
        let cx = ExecContext {
            refs: vec![b"a".as_slice(), b"b".as_slice()],
            residuals: vec![],
        };
        let mut b = bounds();
        b.max_refs = 1;
        assert_eq!(execute(&p, &cx, &b), Err(ExecError::BadRef));
    }

    #[test]
    fn a_reference_cycle_is_detected() {
        let p = Program {
            nodes: vec![n(Op::Ref {
                target: RefTarget::Node(NodeId(0)),
            })],
            literals: vec![],
            root: NodeId(0),
        };
        assert_eq!(run(&p, &ExecContext::new()), Err(ExecError::RefCycle));
    }

    #[test]
    fn a_mutual_reference_cycle_is_detected() {
        let p = Program {
            nodes: vec![
                n(Op::Ref {
                    target: RefTarget::Node(NodeId(1)),
                }),
                n(Op::Slice {
                    src: NodeId(0),
                    from: 0,
                    len: 1,
                }),
            ],
            literals: vec![],
            root: NodeId(0),
        };
        assert_eq!(run(&p, &ExecContext::new()), Err(ExecError::RefCycle));
    }

    #[test]
    fn a_missing_material_reference_is_typed() {
        let p = Program {
            nodes: vec![n(Op::Ref {
                target: RefTarget::Material(3),
            })],
            literals: vec![],
            root: NodeId(0),
        };
        assert_eq!(run(&p, &ExecContext::new()), Err(ExecError::BadRef));
    }

    #[test]
    fn a_slice_past_the_source_end_is_typed() {
        let p = Program {
            nodes: vec![
                n(Op::Literal(LiteralId(0))),
                n(Op::Slice {
                    src: NodeId(0),
                    from: 1,
                    len: 5,
                }),
            ],
            literals: vec![b"ab".to_vec()],
            root: NodeId(1),
        };
        assert_eq!(run(&p, &ExecContext::new()), Err(ExecError::BadRef));
    }

    #[test]
    fn a_slot_outside_the_current_template_is_typed() {
        let p = Program {
            nodes: vec![n(Op::Ref {
                target: RefTarget::Slot(0),
            })],
            literals: vec![],
            root: NodeId(0),
        };
        // No enclosing Template is active, so slot 0 names nothing.
        assert_eq!(run(&p, &ExecContext::new()), Err(ExecError::BadRef));
    }

    #[test]
    fn an_illegal_rank_is_bad_rank_not_malformed() {
        // RankedMismatch gets its own rejection: an out-of-space rank is a state
        // coding error, not a shape mismatch.
        let p = Program {
            nodes: vec![
                n(Op::Literal(LiteralId(0))),
                n(Op::Patch {
                    base: NodeId(0),
                    residual: ResidualId(0),
                }),
            ],
            literals: vec![b"abc".to_vec()],
            root: NodeId(1),
        };
        let cx = ExecContext {
            refs: vec![],
            residuals: vec![Residual::RankedMismatch {
                mask_rank: 999,
                values: vec![b'X'],
            }],
        };
        assert_eq!(run(&p, &cx), Err(ExecError::BadRank));
    }

    #[test]
    fn a_residual_that_does_not_fit_is_malformed() {
        let p = Program {
            nodes: vec![
                n(Op::Literal(LiteralId(0))),
                n(Op::Patch {
                    base: NodeId(0),
                    residual: ResidualId(0),
                }),
            ],
            literals: vec![b"abc".to_vec()],
            root: NodeId(1),
        };
        let cx = ExecContext {
            refs: vec![],
            residuals: vec![Residual::SparseSubstitute {
                positions: vec![10],
                values: vec![b'X'],
            }],
        };
        assert_eq!(run(&p, &cx), Err(ExecError::Malformed));
    }

    #[test]
    fn a_patch_residual_reference_outside_the_pool_is_bad_ref() {
        let p = Program {
            nodes: vec![
                n(Op::Literal(LiteralId(0))),
                n(Op::Patch {
                    base: NodeId(0),
                    residual: ResidualId(9),
                }),
            ],
            literals: vec![b"abc".to_vec()],
            root: NodeId(1),
        };
        assert_eq!(run(&p, &ExecContext::new()), Err(ExecError::BadRef));
    }

    #[test]
    fn a_nested_template_sees_only_its_own_slots() {
        // Inner template's slot 0 shadows the outer's; the outer is not reachable
        // from the inner skeleton, which is the documented scoping rule.
        let p = Program {
            nodes: vec![
                n(Op::Literal(LiteralId(0))), // 0: outer value "O"
                n(Op::Literal(LiteralId(1))), // 1: inner value "I"
                n(Op::Ref {
                    target: RefTarget::Slot(0),
                }), // 2: inner skeleton hole
                n(Op::Template {
                    program: NodeId(2),
                    slots: vec![NodeId(1)],
                }), // 3: inner template -> "I"
                n(Op::Template {
                    program: NodeId(3),
                    slots: vec![NodeId(0)],
                }), // 4: outer template
            ],
            literals: vec![b"O".to_vec(), b"I".to_vec()],
            root: NodeId(4),
        };
        assert_eq!(run(&p, &ExecContext::new()).unwrap(), b"I");
    }
}
