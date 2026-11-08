//! Building THIR from HIR and the type checking results (rustc's
//! `thir/cx`).

use vsc_ast::{BinOpKind, BorrowKind as HirBorrowKind, UnOp};
use vsc_data_structures::FxHashSet;
use vsc_hir::def::{CtorKind, DefKind, Res};
use vsc_hir::intravisit::{self, Visitor};
use vsc_hir::{self as hir, HirId, LangItem};
use vsc_middle::hir::map::Node;
use vsc_middle::thir::*;
use vsc_middle::ty::{self, Adjust, Mutability, Ty, TyCtxt, TyKind, TypeckResults};
use vsc_span::def_id::{DefId, LocalDefId};

pub fn thir_body<'tcx>(tcx: TyCtxt<'tcx>, def: DefId) -> Thir<'tcx> {
    let local = def.expect_local();
    let (body_id, closure) = match tcx.hir_node(local) {
        Some(Node::Closure(c)) => (c.body, Some(local)),
        _ => (crate::body_of(tcx, local).expect("thir_body of a definition without a body"), None),
    };
    let body = tcx.hir_crate.body(body_id);
    let typeck_results = tcx.typeck(def);
    let mut cx = Cx { tcx, results: typeck_results, thir: Thir::new(local), closure, locals: FxHashSet::default() };
    let mut collect = CollectBindings { out: &mut cx.locals };
    collect.visit_body(body);
    let params = body
        .params
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let ty = typeck_results.node_type(p.pat.hir_id).unwrap_or(tcx.types.error);
            Param { pat: Some(Box::new(cx.lower_pattern(&p.pat))), ty, self_kind: i == 0 && is_self_param(tcx, local) }
        })
        .collect();
    cx.thir.params = params;
    let value = cx.mirror_expr(&body.value);
    cx.thir.value = Some(value);
    cx.thir
}

fn is_self_param(tcx: TyCtxt<'_>, def: LocalDefId) -> bool {
    matches!(tcx.def_kind(def.to_def_id()), DefKind::AssocFn)
        && tcx.associated_items(tcx.parent(def.to_def_id())).iter().any(|i| i.def_id == def.to_def_id() && matches!(i.kind, ty::AssocKind::Fn { has_self: true }))
}

/// The bindings a body declares, not counting nested function literals'.
struct CollectBindings<'a> {
    out: &'a mut FxHashSet<HirId>,
}

impl<'v> Visitor<'v> for CollectBindings<'_> {
    fn visit_pat(&mut self, p: &'v hir::Pat) {
        if let hir::PatKind::Binding(_, id, ..) = p.kind {
            self.out.insert(id);
        }
        intravisit::walk_pat(self, p)
    }
}

pub(crate) struct Cx<'tcx> {
    pub tcx: TyCtxt<'tcx>,
    pub results: &'tcx TypeckResults<'tcx>,
    pub thir: Thir<'tcx>,
    /// The function literal being built, if any.
    closure: Option<LocalDefId>,
    /// This body's own bindings: other locals are captured.
    locals: FxHashSet<HirId>,
}

impl<'tcx> Cx<'tcx> {
    fn node_ty(&self, id: HirId) -> Ty<'tcx> {
        self.results.node_type(id).unwrap_or(self.tcx.types.error)
    }

    fn push(&mut self, kind: ExprKind<'tcx>, ty: Ty<'tcx>, span: vsc_span::Span) -> ExprId {
        self.thir.exprs.push(Expr { kind, ty, span })
    }

    pub fn mirror_exprs(&mut self, es: &[hir::Expr]) -> Vec<ExprId> {
        es.iter().map(|e| self.mirror_expr(e)).collect()
    }

    /// An expression, with its adjustments applied on top.
    pub fn mirror_expr(&mut self, e: &hir::Expr) -> ExprId {
        let mut id = self.mirror_unadjusted(e);
        if let Some(adjs) = self.results.adjustments.get(&e.hir_id) {
            for adj in adjs {
                let kind = match adj.kind {
                    Adjust::NeverToAny => ExprKind::NeverToAny { source: id },
                    Adjust::Deref => ExprKind::Deref { arg: id },
                    Adjust::Borrow(Mutability::Not) => ExprKind::Borrow { borrow_kind: BorrowKind::Shared, arg: id },
                    Adjust::Borrow(Mutability::Mut) => ExprKind::Borrow { borrow_kind: BorrowKind::Mut, arg: id },
                    Adjust::Unsize => ExprKind::PointerCoercion { cast: PointerCoercion::Unsize, source: id },
                    Adjust::ReifyFnPointer => ExprKind::PointerCoercion { cast: PointerCoercion::ReifyFnPointer, source: id },
                    Adjust::ToFunc => ExprKind::PointerCoercion { cast: PointerCoercion::ToFunc, source: id },
                };
                id = self.push(kind, adj.target, e.span);
            }
        }
        id
    }

    /// What a path in an expression refers to, after type checking.
    fn path_res(&self, q: &hir::QPath, id: HirId) -> Option<Res<HirId>> {
        match q {
            hir::QPath::Resolved(_, p) => Some(p.res),
            hir::QPath::TypeRelative(..) => self.results.type_dependent_defs.get(&id).map(|&(k, d)| Res::Def(k, d)),
            hir::QPath::LangItem(item, _) => {
                let d = self.tcx.lang_items().get(*item)?;
                let kind = self.tcx.def_kind(d);
                // A variant used as a value is its constructor.
                if kind == DefKind::Variant {
                    let adt = self.tcx.adt_def(self.tcx.parent(d));
                    let v = adt.variants().iter().find(|v| v.def_id == d)?;
                    return match v.ctor {
                        Some((k, c)) => Some(Res::Def(DefKind::Ctor(vsc_hir::def::CtorOf::Variant, k), c)),
                        None => Some(Res::Def(kind, d)),
                    };
                }
                Some(Res::Def(kind, d))
            }
        }
    }

    /// The ADT, variant and arguments a constructor builds, from its type.
    fn ctor_target(&self, ctor: DefId, ty: Ty<'tcx>) -> Option<(ty::AdtDef<'tcx>, usize, ty::GenericArgsRef<'tcx>)> {
        let parent = self.tcx.parent(ctor);
        let (adt_def_id, variant) = if self.tcx.def_kind(parent) == DefKind::Variant { (self.tcx.parent(parent), parent) } else { (parent, parent) };
        let adt = self.tcx.adt_def(adt_def_id);
        let index = adt.variants().iter().position(|v| v.def_id == variant).unwrap_or(0);
        let args = match *ty.kind() {
            TyKind::Adt(_, args) => args,
            TyKind::FnDef(_, args) => args,
            _ => return None,
        };
        Some((adt, index, args))
    }

    fn mirror_unadjusted(&mut self, e: &hir::Expr) -> ExprId {
        let tcx = self.tcx;
        let ty = self.node_ty(e.hir_id);
        let kind = match &e.kind {
            hir::ExprKind::Lit(lit) => ExprKind::Literal { lit: *lit, neg: false },
            hir::ExprKind::Unary(UnOp::Neg, inner) if matches!(&inner.kind, hir::ExprKind::Lit(_)) => {
                let hir::ExprKind::Lit(lit) = &inner.kind else { unreachable!() };
                ExprKind::Literal { lit: *lit, neg: true }
            }
            hir::ExprKind::Path(q) => self.path_expr(q, e.hir_id, ty),
            hir::ExprKind::Call(callee, args) => {
                let ctor = match &callee.kind {
                    hir::ExprKind::Path(q) => match self.path_res(q, callee.hir_id) {
                        Some(Res::Def(DefKind::Ctor(_, CtorKind::Fn), c)) => Some(c),
                        Some(Res::SelfCtor(imp)) => match *tcx.type_of(imp).kind() {
                            TyKind::Adt(adt, _) if !adt.is_enum() => adt.non_enum_variant().ctor.map(|(_, c)| c),
                            _ => None,
                        },
                        _ => None,
                    },
                    _ => None,
                };
                match ctor.and_then(|c| self.ctor_target(c, ty)) {
                    // `Some(x)`, `Point(1, 2)`: building a value.
                    Some((adt_def, variant_index, generic_args)) => {
                        let fields = args_fields(self.mirror_exprs(args));
                        ExprKind::Adt(Box::new(AdtExpr { adt_def, variant_index, args: generic_args, fields, base: None }))
                    }
                    None => {
                        let fun = self.mirror_expr(callee);
                        ExprKind::Call { fun, args: self.mirror_exprs(args), fn_span: e.span }
                    }
                }
            }
            hir::ExprKind::MethodCall(_, recv, args, span) => {
                // A method call is a call of the method's function item.
                let (fun_ty, ok) = match (self.results.type_dependent_defs.get(&e.hir_id), self.results.node_args.get(&e.hir_id)) {
                    (Some(&(_, def)), Some(&args)) => (tcx.mk_fn_def(def, args), true),
                    _ => (tcx.types.error, false),
                };
                let fun = self.push(if ok { ExprKind::ZstLiteral } else { ExprKind::Error }, fun_ty, *span);
                let mut all = vec![self.mirror_expr(recv)];
                all.extend(self.mirror_exprs(args));
                ExprKind::Call { fun, args: all, fn_span: e.span }
            }
            hir::ExprKind::Tup(es) => ExprKind::Tuple { fields: self.mirror_exprs(es) },
            hir::ExprKind::Array(es) => ExprKind::Array { fields: self.mirror_exprs(es) },
            hir::ExprKind::Repeat(x, _) => {
                let count = match *ty.kind() {
                    TyKind::Array(_, n) => n,
                    _ => tcx.mk_const(ty::ConstKind::Error),
                };
                ExprKind::Repeat { value: self.mirror_expr(x), count }
            }
            hir::ExprKind::Binary(op, a, b) => match op.node {
                BinOpKind::And => ExprKind::LogicalOp { op: LogicalOp::And, lhs: self.mirror_expr(a), rhs: self.mirror_expr(b) },
                BinOpKind::Or => ExprKind::LogicalOp { op: LogicalOp::Or, lhs: self.mirror_expr(a), rhs: self.mirror_expr(b) },
                op => ExprKind::Binary { op, lhs: self.mirror_expr(a), rhs: self.mirror_expr(b) },
            },
            hir::ExprKind::AssignOp(op, a, b) => ExprKind::AssignOp { op: op.node, lhs: self.mirror_expr(a), rhs: self.mirror_expr(b) },
            hir::ExprKind::Assign(a, b, _) => ExprKind::Assign { lhs: self.mirror_expr(a), rhs: self.mirror_expr(b) },
            hir::ExprKind::Unary(UnOp::Deref, x) => ExprKind::Deref { arg: self.mirror_expr(x) },
            hir::ExprKind::Unary(op, x) => ExprKind::Unary { op: *op, arg: self.mirror_expr(x) },
            hir::ExprKind::Cast(x, _) => ExprKind::Cast { source: self.mirror_expr(x) },
            hir::ExprKind::DropTemps(x) => ExprKind::Use { source: self.mirror_expr(x) },
            hir::ExprKind::Let(l) => {
                let expr = self.mirror_expr(&l.init);
                ExprKind::Let { expr, pat: Box::new(self.lower_pattern(&l.pat)) }
            }
            hir::ExprKind::If(c, t, els) => ExprKind::If {
                cond: self.mirror_expr(c),
                then: self.mirror_expr(t),
                else_opt: els.as_ref().map(|x| self.mirror_expr(x)),
            },
            hir::ExprKind::Loop(b, ..) => {
                let block = self.mirror_block(b);
                let body = self.push(ExprKind::Block { block }, tcx.types.unit, b.span);
                ExprKind::Loop { body, id: e.hir_id }
            }
            hir::ExprKind::Match(scrutinee, arms, source) => {
                let scrutinee = self.mirror_expr(scrutinee);
                let arms = arms
                    .iter()
                    .map(|a| {
                        let arm = Arm {
                            pattern: Box::new(self.lower_pattern(&a.pat)),
                            guard: a.guard.as_ref().map(|g| self.mirror_expr(g)),
                            body: self.mirror_expr(&a.body),
                            span: a.span,
                        };
                        self.thir.arms.push(arm)
                    })
                    .collect();
                let source = match source {
                    hir::MatchSource::Normal => MatchSource::Normal,
                    hir::MatchSource::ForLoopDesugar => MatchSource::ForLoop,
                    hir::MatchSource::TryDesugar => MatchSource::Try,
                    hir::MatchSource::FormatArgs => MatchSource::Desugared,
                };
                ExprKind::Match { scrutinee, arms, source }
            }
            hir::ExprKind::Closure(c) => ExprKind::Closure { closure_id: c.def_id },
            hir::ExprKind::Block(b, label) => {
                let block = self.mirror_block(b);
                if label.is_some() {
                    self.thir.blocks[block].target = Some(e.hir_id);
                }
                ExprKind::Block { block }
            }
            hir::ExprKind::Field(base, _) => {
                let lhs = self.mirror_expr(base);
                let field = self.results.field_indices.get(&e.hir_id).copied().unwrap_or(0);
                ExprKind::Field { lhs, variant_index: 0, field }
            }
            hir::ExprKind::Index(base, idx, _) => ExprKind::Index { lhs: self.mirror_expr(base), index: self.mirror_expr(idx) },
            hir::ExprKind::AddrOf(HirBorrowKind::Ref, m, x) => ExprKind::Borrow {
                borrow_kind: if m.is_mut() { BorrowKind::Mut } else { BorrowKind::Shared },
                arg: self.mirror_expr(x),
            },
            hir::ExprKind::AddrOf(HirBorrowKind::Raw, m, x) => ExprKind::RawBorrow { mutability: *m, arg: self.mirror_expr(x) },
            hir::ExprKind::Break(dest, value) => match dest.target_id {
                Some(target) => ExprKind::Break { target, value: value.as_ref().map(|v| self.mirror_expr(v)) },
                None => ExprKind::Error,
            },
            hir::ExprKind::Continue(dest) => match dest.target_id {
                Some(target) => ExprKind::Continue { target },
                None => ExprKind::Error,
            },
            hir::ExprKind::Ret(v) => ExprKind::Return { value: v.as_ref().map(|v| self.mirror_expr(v)) },
            hir::ExprKind::InlineAsm(asm) => {
                let mut operands = Vec::new();
                for (op, _) in &asm.operands {
                    match op {
                        hir::InlineAsmOperand::In { expr, .. } | hir::InlineAsmOperand::InOut { expr, .. } | hir::InlineAsmOperand::SymFn { expr } => {
                            operands.push(self.mirror_expr(expr))
                        }
                        hir::InlineAsmOperand::Out { expr, .. } => operands.extend(expr.as_ref().map(|x| self.mirror_expr(x))),
                        hir::InlineAsmOperand::SplitInOut { in_expr, out_expr, .. } => {
                            operands.push(self.mirror_expr(in_expr));
                            operands.extend(out_expr.as_ref().map(|x| self.mirror_expr(x)));
                        }
                        hir::InlineAsmOperand::Label { block } => {
                            let b = self.mirror_block(block);
                            operands.push(self.push(ExprKind::Block { block: b }, tcx.types.unit, block.span));
                        }
                        hir::InlineAsmOperand::Const { .. } => {}
                    }
                }
                ExprKind::InlineAsm { operands }
            }
            hir::ExprKind::Struct(q, fields, tail) => return self.struct_expr(e, q, fields, tail, ty),
            hir::ExprKind::ConstBlock(c) => {
                let body = tcx.hir_crate.body(c.body);
                return self.mirror_expr(&body.value);
            }
            hir::ExprKind::Await(_) | hir::ExprKind::Err => ExprKind::Error,
        };
        self.push(kind, ty, e.span)
    }

    fn path_expr(&mut self, q: &hir::QPath, id: HirId, ty: Ty<'tcx>) -> ExprKind<'tcx> {
        let tcx = self.tcx;
        match self.path_res(q, id) {
            Some(Res::Local(var)) => {
                if self.locals.contains(&var) {
                    ExprKind::VarRef { id: var }
                } else {
                    match self.closure {
                        Some(c) => ExprKind::UpvarRef { closure_def_id: c.to_def_id(), var_hir_id: var },
                        None => ExprKind::VarRef { id: var },
                    }
                }
            }
            Some(Res::Def(DefKind::Fn | DefKind::AssocFn | DefKind::Ctor(_, CtorKind::Fn), _)) => ExprKind::ZstLiteral,
            Some(Res::Def(DefKind::Ctor(_, CtorKind::Const), c)) => match self.ctor_target(c, ty) {
                Some((adt_def, variant_index, args)) => {
                    ExprKind::Adt(Box::new(AdtExpr { adt_def, variant_index, args, fields: Vec::new(), base: None }))
                }
                None => ExprKind::Error,
            },
            Some(Res::SelfCtor(_)) => match *ty.kind() {
                TyKind::Adt(adt_def, args) => ExprKind::Adt(Box::new(AdtExpr { adt_def, variant_index: 0, args, fields: Vec::new(), base: None })),
                TyKind::FnDef(..) => ExprKind::ZstLiteral,
                _ => ExprKind::Error,
            },
            Some(Res::Def(DefKind::Const | DefKind::AssocConst, def)) => {
                ExprKind::NamedConst { def_id: def, args: self.results.node_args.get(&id).copied().unwrap_or(&[]) }
            }
            Some(Res::Def(DefKind::ConstParam, def)) => ExprKind::ConstParam { def_id: def },
            Some(Res::Def(DefKind::Static { .. }, def)) => ExprKind::StaticRef { def_id: def },
            // A unit struct lang item (`RangeFull`).
            Some(Res::Def(DefKind::Struct, d)) => match *ty.kind() {
                TyKind::Adt(adt_def, args) if adt_def.did() == d => {
                    ExprKind::Adt(Box::new(AdtExpr { adt_def, variant_index: 0, args, fields: Vec::new(), base: None }))
                }
                _ => ExprKind::Error,
            },
            _ => {
                let _ = tcx;
                ExprKind::Error
            }
        }
    }

    fn struct_expr(&mut self, e: &hir::Expr, q: &hir::QPath, fields: &[hir::ExprField], tail: &hir::StructTailExpr, ty: Ty<'tcx>) -> ExprId {
        let tcx = self.tcx;
        // A class literal builds the class's struct, then the class value.
        let (inner_ty, is_class) = match *ty.kind() {
            TyKind::Adt(d, args) if tcx.is_lang_item(d.did(), LangItem::Class) && args.len() == 1 => (args[0].expect_ty(), true),
            _ => (ty, false),
        };
        let TyKind::Adt(adt_def, args) = *inner_ty.kind() else { return self.push(ExprKind::Error, ty, e.span) };
        let variant_index = match q {
            hir::QPath::Resolved(_, p) => match p.res {
                Res::Def(DefKind::Variant, v) => adt_def.variants().iter().position(|x| x.def_id == v).unwrap_or(0),
                _ => 0,
            },
            hir::QPath::TypeRelative(_, seg) if adt_def.is_enum() => adt_def.variants().iter().position(|v| v.name == seg.ident.name).unwrap_or(0),
            _ => 0,
        };
        let fields: Vec<(usize, ExprId)> =
            fields.iter().map(|f| (self.results.field_indices.get(&f.hir_id).copied().unwrap_or(0), self.mirror_expr(&f.expr))).collect();
        let base = match tail {
            hir::StructTailExpr::Base(b) => Some(self.mirror_expr(b)),
            hir::StructTailExpr::None => None,
        };
        let value = self.push(ExprKind::Adt(Box::new(AdtExpr { adt_def, variant_index, args, fields, base })), inner_ty, e.span);
        if is_class { self.push(ExprKind::ClassNew { value }, ty, e.span) } else { value }
    }

    pub fn mirror_block(&mut self, b: &hir::Block) -> BlockId {
        let mut stmts = Vec::new();
        for s in &b.stmts {
            let kind = match &s.kind {
                hir::StmtKind::Let(l) => {
                    let initializer = l.init.as_ref().map(|i| self.mirror_expr(i));
                    let else_block = l.els.as_ref().map(|b| self.mirror_block(b));
                    StmtKind::Let { pattern: Box::new(self.lower_pattern(&l.pat)), initializer, else_block, span: l.span }
                }
                hir::StmtKind::Semi(e) => StmtKind::Expr { expr: self.mirror_expr(e) },
                hir::StmtKind::Item(_) => continue,
            };
            stmts.push(self.thir.stmts.push(Stmt { kind }));
        }
        let expr = b.expr.as_ref().map(|e| self.mirror_expr(e));
        let safety_mode = match b.rules {
            hir::BlockCheckMode::Unsafe => BlockSafety::ExplicitUnsafe(b.hir_id),
            _ => BlockSafety::Safe,
        };
        self.thir.blocks.push(Block { target: None, stmts, expr, safety_mode, span: b.span })
    }
}

/// Call arguments as fields 0, 1, ….
fn args_fields(args: Vec<ExprId>) -> Vec<(usize, ExprId)> {
    args.into_iter().enumerate().collect()
}
