//! Inserts [`StatementKind::StorageLive`] / [`StatementKind::StorageDead`]
//! markers for user variables and temporaries when not yet present.

use vsc_index::Idx;
use vsc_middle::mir::*;

pub fn run(body: &mut Body<'_>) {
    if body.basic_blocks.is_empty() {
        return;
    }

    let mut has_storage = vec![false; body.local_decls.len()];
    for data in &body.basic_blocks {
        for stmt in &data.statements {
            if let StatementKind::StorageLive(l) | StatementKind::StorageDead(l) = stmt.kind {
                has_storage[l.index()] = true;
            }
        }
    }

    let managed: Vec<Local> = (body.arg_count + 1..body.local_decls.len())
        .map(Local::from_usize)
        .filter(|l| !has_storage[l.index()])
        .collect();

    if managed.is_empty() {
        return;
    }

    let span = body.span;
    let live_stmts: Vec<Statement<'_>> =
        managed.iter().map(|&l| Statement { span, kind: StatementKind::StorageLive(l) }).collect();
    body.basic_blocks[START_BLOCK].statements.splice(0..0, live_stmts);

    for data in &mut body.basic_blocks {
        if matches!(data.terminator().kind, TerminatorKind::Return) {
            let term_span = data.terminator().span;
            for &l in managed.iter().rev() {
                data.statements.push(Statement {
                    span: term_span,
                    kind: StatementKind::StorageDead(l),
                });
            }
        }
    }
}
