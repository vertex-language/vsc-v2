//! Lightweight MIR cleanup passes: trivial self-copy removal, constant switch
//! folding, jump threading, linear block merging, and unreachable block pruning.

use vsc_index::{IndexVec, Idx};
use vsc_middle::mir::*;

/// Removes self-assignments `_x = Use(Copy(_x) | Move(_x))` and `Nop`s.
pub fn remove_redundant_copies(body: &mut Body<'_>) {
    for data in &mut body.basic_blocks {
        data.statements.retain(|stmt| match &stmt.kind {
            StatementKind::Nop => false,
            StatementKind::Assign(box_pair) => {
                let (lhs, rv) = &**box_pair;
                if let Rvalue::Use(Operand::Copy(rhs) | Operand::Move(rhs)) = rv {
                    lhs != rhs
                } else {
                    true
                }
            }
            _ => true,
        });
    }
}

/// Simplifies the CFG and compacts basic block indices so every block is reachable
/// from [`START_BLOCK`].
pub fn simplify_cfg(body: &mut Body<'_>) {
    if body.basic_blocks.is_empty() {
        return;
    }

    // 1. Constant-fold SwitchInt with constant discriminant.
    for data in &mut body.basic_blocks {
        let term = data.terminator.as_mut().unwrap();
        if let TerminatorKind::SwitchInt {
            discr: Operand::Constant(c),
            targets,
        } = &term.kind
            && let ConstValue::Scalar(val) = c.value
        {
            let target = targets
                .values
                .iter()
                .find(|(v, _)| *v == val)
                .map_or(targets.otherwise, |(_, t)| *t);
            term.kind = TerminatorKind::Goto { target };
        }
    }

    // 2. Iteratively thread empty Goto blocks and merge single-predecessor linear chains.
    loop {
        let mut changed = false;

        // Deduplicate SwitchInt targets where all arms go to the same target.
        for data in &mut body.basic_blocks {
            let term = data.terminator.as_mut().unwrap();
            if let TerminatorKind::SwitchInt { targets, .. } = &mut term.kind {
                let otherwise = targets.otherwise;
                targets.values.retain(|&(_, t)| t != otherwise);
                if targets.values.is_empty() {
                    term.kind = TerminatorKind::Goto { target: otherwise };
                    changed = true;
                }
            }
        }

        // Thread edges through empty Goto blocks.
        let n = body.basic_blocks.len();
        let mut redirect = vec![None; n];
        for (bb, data) in body.basic_blocks.iter_enumerated() {
            if bb != START_BLOCK
                && data.statements.is_empty()
                && let TerminatorKind::Goto { target } = data.terminator().kind
                && target != bb
            {
                redirect[bb.index()] = Some(target);
            }
        }
        for data in &mut body.basic_blocks {
            let term = data.terminator.as_mut().unwrap();
            for succ in term.kind.successors_mut() {
                let mut cur = *succ;
                let mut steps = 0;
                while let Some(next) = redirect[cur.index()] {
                    cur = next;
                    steps += 1;
                    if steps >= n {
                        break;
                    }
                }
                if cur != *succ {
                    *succ = cur;
                    changed = true;
                }
            }
        }

        // Merge `bb -> Goto { target: succ }` when `succ` has `bb` as its sole predecessor.
        let mut pred_counts = vec![0usize; n];
        for data in &body.basic_blocks {
            for succ in data.terminator().kind.successors() {
                pred_counts[succ.index()] += 1;
            }
        }

        for bb_idx in 0..n {
            let bb = BasicBlock::from_usize(bb_idx);
            let target = match body.basic_blocks[bb].terminator().kind {
                TerminatorKind::Goto { target }
                    if target != bb
                        && target != START_BLOCK
                        && pred_counts[target.index()] == 1 =>
                {
                    target
                }
                _ => continue,
            };
            let mut next_stmts = std::mem::take(&mut body.basic_blocks[target].statements);
            let next_term = body.basic_blocks[target].terminator.take().unwrap();
            body.basic_blocks[bb].statements.append(&mut next_stmts);
            body.basic_blocks[bb].terminator = Some(next_term);
            body.basic_blocks[target].terminator =
                Some(Terminator { span: body.span, kind: TerminatorKind::Unreachable });
            pred_counts[target.index()] = 0;
            changed = true;
        }

        if !changed {
            break;
        }
    }

    // 3. Remove unreachable blocks and compact indices.
    let n = body.basic_blocks.len();
    let mut visited = vec![false; n];
    let mut stack = vec![START_BLOCK];
    visited[START_BLOCK.index()] = true;
    while let Some(bb) = stack.pop() {
        for succ in body.basic_blocks[bb].terminator().kind.successors() {
            if !visited[succ.index()] {
                visited[succ.index()] = true;
                stack.push(succ);
            }
        }
    }

    let mut remap: Vec<Option<BasicBlock>> = vec![None; n];
    let mut new_blocks = IndexVec::new();
    let old_blocks = std::mem::take(&mut body.basic_blocks);
    for (old_bb, data) in old_blocks.into_iter().enumerate() {
        if visited[old_bb] {
            let new_bb = new_blocks.push(data);
            remap[old_bb] = Some(new_bb);
        }
    }
    for data in &mut new_blocks {
        for succ in data.terminator.as_mut().unwrap().kind.successors_mut() {
            *succ = remap[succ.index()].expect("reachable edge must target reachable block");
        }
    }
    body.basic_blocks = new_blocks;
}
