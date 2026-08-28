//! Escape analysis: stack-allocatable locals, class drops, and copy edges.

use super::*;

/// How an expression's value is consumed by its parent node.
#[derive(Clone, Copy, PartialEq)]
pub(crate) enum ExprCtx {
    /// The value is used as data (an argument, operand, return value, ...).
    Data,

    /// The value is dereferenced further (the object of `Member`/`Index`/...).
    Base,
}

/// Collect every local `var x = ClassName()` that could be stack-allocated:
/// a zero-argument construction of an `init`-less class, plus `var y = x`
/// aliases of already-collected candidates.
pub(crate) fn collect_stack_candidates(
    body: &[Stmt],
    candidates: &mut HashSet<String>,
    module: &Module<'_>,
) {
    for stmt in body {
        if let Stmt::Var {
            name,
            type_annotation,
            initializer,
            is_static,
            is_const: _,
            view,
        } = stmt
        {
            if *is_static || type_annotation.is_some() || view.is_some() {
                continue;
            }
            let Some(init) = initializer else {
                continue;
            };
            let eligible = match init {
                Expr::Call {
                    callee, arguments, ..
                } if arguments.is_empty() => {
                    matches!(callee.as_ref(), Expr::Variable { name: ctor }
                        if module.get_struct_type(ctor.lexeme()).is_some()
                            && module
                                .get_function(&format!("{}.init", ctor.lexeme()))
                                .is_none())
                }
                Expr::Variable { name: alias } => candidates.contains(alias.lexeme()),
                _ => false,
            };
            if eligible {
                candidates.insert(name.lexeme().to_string());
            }
        }
    }
}

pub(crate) fn expr_mentions_var(expr: &Expr, var: &str) -> bool {
    match expr {
        Expr::Variable { name } => name.lexeme() == var,
        Expr::Literal { .. } | Expr::This { .. } => false,
        Expr::Binary { left, right, .. } => {
            expr_mentions_var(left, var) || expr_mentions_var(right, var)
        }
        Expr::Unary { right, .. } | Expr::Spread { value: right, .. } => {
            expr_mentions_var(right, var)
        }
        Expr::PostfixUnary { left, .. } => expr_mentions_var(left, var),
        Expr::Grouping { expression, .. } => expr_mentions_var(expression, var),
        Expr::Member { object, .. } | Expr::OptionalMember { object, .. } => {
            expr_mentions_var(object, var)
        }
        Expr::Call {
            callee, arguments, ..
        }
        | Expr::Await {
            callee, arguments, ..
        } => expr_mentions_var(callee, var) || arguments.iter().any(|a| expr_mentions_var(a, var)),
        Expr::AsyncBlock { body, .. } => body.iter().any(|s| stmt_mentions_var(s, var)),
        Expr::Assign { name, value } => name.lexeme() == var || expr_mentions_var(value, var),
        Expr::IndexGet { object, index } => {
            expr_mentions_var(object, var) || expr_mentions_var(index, var)
        }
        Expr::IndexSet {
            object,
            index,
            value,
        } => {
            expr_mentions_var(object, var)
                || expr_mentions_var(index, var)
                || expr_mentions_var(value, var)
        }
        Expr::MemberSet { object, value, .. } => {
            expr_mentions_var(object, var) || expr_mentions_var(value, var)
        }
        Expr::Lambda { body, .. } => body.iter().any(|s| stmt_mentions_var(s, var)),
        Expr::Ternary {
            condition,
            then_branch,
            else_branch,
        } => {
            expr_mentions_var(condition, var)
                || expr_mentions_var(then_branch, var)
                || expr_mentions_var(else_branch, var)
        }
        Expr::ObjectLiteral { properties, .. } => {
            properties.iter().any(|p| expr_mentions_var(&p.value, var))
        }
        Expr::ArrayLiteral { elements, .. } => elements.iter().any(|e| expr_mentions_var(e, var)),
        Expr::View { target, .. } => expr_mentions_var(target, var),
        Expr::Copy { expression, .. } => expr_mentions_var(expression, var),
        Expr::Propagate { value, .. } => expr_mentions_var(value, var),
        Expr::Borrow { target, .. } | Expr::RawDeref { target, .. } => {
            expr_mentions_var(target, var)
        }
        Expr::RawDerefSet { target, value, .. } => {
            expr_mentions_var(target, var) || expr_mentions_var(value, var)
        }
        Expr::StructLiteral {
            class_name: _,
            fields,
            update,
            ..
        } => {
            fields.iter().any(|p| expr_mentions_var(&p.value, var))
                || update.as_ref().is_some_and(|u| expr_mentions_var(u, var))
        }
        Expr::TupleLiteral { elements, .. } => elements.iter().any(|e| expr_mentions_var(e, var)),
        Expr::TupleIndex { object, .. } => expr_mentions_var(object, var),
        Expr::ChanSend { channel, value, .. } => {
            expr_mentions_var(channel, var) || expr_mentions_var(value, var)
        }
        Expr::ChanRecv { channel, .. } => expr_mentions_var(channel, var),
        Expr::Close { channel, .. } => expr_mentions_var(channel, var),
    }
}

pub(crate) fn stmt_mentions_var(stmt: &Stmt, var: &str) -> bool {
    match stmt {
        Stmt::Expression { expression } | Stmt::Say { expression, .. } => {
            expr_mentions_var(expression, var)
        }
        Stmt::Var {
            initializer: Some(init),
            ..
        } => expr_mentions_var(init, var),
        Stmt::Var { .. }
        | Stmt::Break { .. }
        | Stmt::Continue { .. }
        | Stmt::Function { .. }
        | Stmt::AsyncFunction { .. }
        | Stmt::Class { .. }
        | Stmt::Use { .. }
        | Stmt::Enum { .. }
        | Stmt::Test { .. }
        | Stmt::Trait { .. }
        | Stmt::Impl { .. } => false,
        Stmt::Block { statements, .. } => statements.iter().any(|s| stmt_mentions_var(s, var)),
        Stmt::If {
            condition,
            then_branch,
            elif_branches,
            else_branch,
        } => {
            expr_mentions_var(condition, var)
                || stmt_mentions_var(then_branch, var)
                || elif_branches
                    .iter()
                    .any(|e| stmt_mentions_var(&e.body, var))
                || else_branch
                    .as_deref()
                    .is_some_and(|s| stmt_mentions_var(s, var))
        }
        Stmt::While { condition, body } | Stmt::DoWhile { body, condition } => {
            expr_mentions_var(condition, var) || stmt_mentions_var(body, var)
        }
        Stmt::For {
            init,
            condition,
            update,
            body,
        } => {
            init.as_deref().is_some_and(|s| stmt_mentions_var(s, var))
                || condition
                    .as_ref()
                    .is_some_and(|e| expr_mentions_var(e, var))
                || update.as_ref().is_some_and(|e| expr_mentions_var(e, var))
                || stmt_mentions_var(body, var)
        }
        Stmt::ForIn { iterable, body, .. } => {
            expr_mentions_var(iterable, var) || stmt_mentions_var(body, var)
        }
        Stmt::ForAwait { producer, body, .. } => {
            expr_mentions_var(producer, var) || stmt_mentions_var(body, var)
        }
        Stmt::Return { value } => value.as_ref().is_some_and(|v| expr_mentions_var(v, var)),
        Stmt::Throw { value } => expr_mentions_var(value, var),
        Stmt::Match {
            expression,
            cases,
            default_case,
        } => {
            expr_mentions_var(expression, var)
                || cases.iter().any(|c| stmt_mentions_var(&c.body, var))
                || default_case
                    .as_deref()
                    .is_some_and(|s| stmt_mentions_var(s, var))
        }
        Stmt::Try {
            try_block,
            catch_block,
            finally_block,
            ..
        } => {
            stmt_mentions_var(try_block, var)
                || catch_block
                    .as_deref()
                    .is_some_and(|s| stmt_mentions_var(s, var))
                || finally_block
                    .as_deref()
                    .is_some_and(|s| stmt_mentions_var(s, var))
        }
        Stmt::Retry {
            count,
            body,
            catch_block,
            ..
        } => {
            expr_mentions_var(count, var)
                || stmt_mentions_var(body, var)
                || catch_block
                    .as_deref()
                    .is_some_and(|s| stmt_mentions_var(s, var))
        }
        Stmt::Unsafe { body } => stmt_mentions_var(body, var),
        Stmt::Quiet { body, .. } => stmt_mentions_var(body, var),
        Stmt::Destructure { initializer, .. } => expr_mentions_var(initializer, var),
        Stmt::TypeAlias { .. } => false,
        Stmt::ChanRecvFor { channel, body, .. } => {
            expr_mentions_var(channel, var) || stmt_mentions_var(body, var)
        }
        Stmt::Go { call, block, .. } => {
            expr_mentions_var(call, var)
                || block
                    .as_ref()
                    .is_some_and(|stmts| stmts.iter().any(|s| stmt_mentions_var(s, var)))
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum EscapeKind {
    StackSlot,

    FieldDrop,
}

/// Returns `false` if `expr` uses `var` in a position that could let the
/// object's address escape the function or alias the instance.
pub(crate) fn expr_uses_var_safely(expr: &Expr, var: &str, ctx: ExprCtx, kind: EscapeKind) -> bool {
    // Expressions that never reference `var` cannot leak it; skip the
    // context-sensitive rules for unrelated subtrees.
    if !expr_mentions_var(expr, var) {
        return true;
    }
    match expr {
        // A bare reference is only safe as a single-level base.
        Expr::Variable { name: _ } => ctx == ExprCtx::Base,
        Expr::Literal { .. } | Expr::This { .. } => true,
        // Grouping an object base is treated as an escape (conservative).
        Expr::Grouping { expression, .. } => {
            expr_uses_var_safely(expression, var, ExprCtx::Data, kind)
        }
        // `var.field`: the field value may be used anywhere but not
        // dereferenced again (no member chains) — a chain could hand out an
        // interior address. How deep a read goes does not affect who owns
        // the instance's fields, so a chain never rejects a field-drop
        // candidate; refusing would just leak the fields of an instance
        // nothing else owns.
        Expr::Member { object, .. } | Expr::OptionalMember { object, .. } => {
            expr_uses_var_safely(object, var, ExprCtx::Base, kind)
                && (ctx == ExprCtx::Data || kind == EscapeKind::FieldDrop)
        }
        // `var[i]` — safe, unless chained further (see `Member`).
        Expr::IndexGet { object, index } => {
            expr_uses_var_safely(object, var, ExprCtx::Base, kind)
                && expr_uses_var_safely(index, var, ExprCtx::Data, kind)
                && (ctx == ExprCtx::Data || kind == EscapeKind::FieldDrop)
        }
        // `var[i] = v` and `var.field = v` write a slot; they never leak
        // the address.
        Expr::IndexSet {
            object,
            index,
            value,
        } => {
            expr_uses_var_safely(object, var, ExprCtx::Base, kind)
                && expr_uses_var_safely(index, var, ExprCtx::Data, kind)
                && expr_uses_var_safely(value, var, ExprCtx::Data, kind)
        }
        Expr::MemberSet { object, value, .. } => {
            expr_uses_var_safely(object, var, ExprCtx::Base, kind)
                && expr_uses_var_safely(value, var, ExprCtx::Data, kind)
        }
        Expr::Call {
            callee, arguments, ..
        } => {
            // `var.method(args)`: the receiver is the object itself, which
            // stays local to the function.
            if let Expr::Member { object, .. } = callee.as_ref()
                && let Expr::Variable { name } = object.as_ref()
                && name.lexeme() == var
            {
                return arguments
                    .iter()
                    .all(|a| expr_uses_var_safely(a, var, ExprCtx::Data, kind));
            }
            expr_uses_var_safely(callee, var, ExprCtx::Base, kind)
                && arguments
                    .iter()
                    .all(|a| expr_uses_var_safely(a, var, ExprCtx::Data, kind))
        }
        Expr::Await {
            callee, arguments, ..
        } => {
            expr_uses_var_safely(callee, var, ExprCtx::Base, kind)
                && arguments
                    .iter()
                    .all(|a| expr_uses_var_safely(a, var, ExprCtx::Data, kind))
        }
        Expr::AsyncBlock { body, .. } => body.iter().all(|s| stmt_uses_var_safely(s, var, kind)),
        Expr::Assign { name, value } => {
            // Reassigning the object would drop its slot-allocated identity.
            name.lexeme() != var && expr_uses_var_safely(value, var, ExprCtx::Data, kind)
        }
        Expr::Binary { left, right, .. } => {
            expr_uses_var_safely(left, var, ExprCtx::Data, kind)
                && expr_uses_var_safely(right, var, ExprCtx::Data, kind)
        }
        Expr::Unary { right, .. } | Expr::Spread { value: right, .. } => {
            expr_uses_var_safely(right, var, ExprCtx::Data, kind)
        }
        Expr::PostfixUnary { left, .. } => expr_uses_var_safely(left, var, ExprCtx::Data, kind),
        Expr::Ternary {
            condition,
            then_branch,
            else_branch,
        } => {
            expr_uses_var_safely(condition, var, ExprCtx::Data, kind)
                && expr_uses_var_safely(then_branch, var, ExprCtx::Data, kind)
                && expr_uses_var_safely(else_branch, var, ExprCtx::Data, kind)
        }
        Expr::ObjectLiteral { properties, .. } => properties
            .iter()
            .all(|p| expr_uses_var_safely(&p.value, var, ExprCtx::Data, kind)),
        Expr::ArrayLiteral { elements, .. } => elements
            .iter()
            .all(|e| expr_uses_var_safely(e, var, ExprCtx::Data, kind)),
        Expr::Lambda { body, .. } => !body.iter().any(|s| stmt_mentions_var(s, var)),
        Expr::View { target, .. } => match kind {
            // A view hands out the instance's address: it counts as an
            // escape, so the object stays on the heap.
            EscapeKind::StackSlot => expr_uses_var_safely(target, var, ExprCtx::Data, kind),

            // A view only borrows: it never becomes an owner, and a view
            // that outlives its source is rejected by the type checker, so
            // this scope is still the only owner — refusing the drop would
            // just leak the fields.
            EscapeKind::FieldDrop => view_borrows_place(target, var, kind),
        },
        Expr::Copy { expression, .. } => expr_uses_var_safely(expression, var, ExprCtx::Base, kind),
        Expr::Propagate { value, .. } => expr_uses_var_safely(value, var, ExprCtx::Data, kind),
        Expr::Borrow { target, .. } | Expr::RawDeref { target, .. } => {
            expr_uses_var_safely(target, var, ExprCtx::Base, kind)
        }
        Expr::RawDerefSet { target, value, .. } => {
            expr_uses_var_safely(target, var, ExprCtx::Base, kind)
                || expr_uses_var_safely(value, var, ExprCtx::Base, kind)
        }
        Expr::StructLiteral {
            class_name: _,
            fields,
            update,
            ..
        } => {
            fields
                .iter()
                .any(|p| expr_uses_var_safely(&p.value, var, ExprCtx::Data, kind))
                // A `..base` update deep-copies the base's fields and never
                // aliases the base, so it reads like `copy(base)`.
                || update
                    .as_ref()
                    .is_some_and(|u| expr_uses_var_safely(u, var, ExprCtx::Base, kind))
        }
        Expr::TupleLiteral { elements, .. } => elements
            .iter()
            .any(|e| expr_uses_var_safely(e, var, ExprCtx::Data, kind)),
        Expr::TupleIndex { object, .. } => expr_uses_var_safely(object, var, ExprCtx::Base, kind),
        // Channel operations are not yet supported by codegen; conservatively
        // refuse to classify them as safe.
        Expr::ChanSend { .. } | Expr::ChanRecv { .. } | Expr::Close { .. } => false,
    }
}

/// Whether a `view` target only borrows a place rooted at `var` — `var`,
/// `var.field`, `var[i]`, and chains of those. Borrowing a place transfers
/// no ownership, so it never rejects a field-drop candidate; parts of the
/// target that are not the place itself (an index expression, a call) are
/// still checked as data.
pub(crate) fn view_borrows_place(expr: &Expr, var: &str, kind: EscapeKind) -> bool {
    match expr {
        Expr::Variable { .. } | Expr::This { .. } => true,
        Expr::Member { object, .. } | Expr::OptionalMember { object, .. } => {
            view_borrows_place(object, var, kind)
        }
        Expr::IndexGet { object, index } => {
            view_borrows_place(object, var, kind)
                && expr_uses_var_safely(index, var, ExprCtx::Data, kind)
        }
        Expr::Grouping { expression, .. } => view_borrows_place(expression, var, kind),
        other => expr_uses_var_safely(other, var, ExprCtx::Data, kind),
    }
}

pub(crate) fn stmt_uses_var_safely(stmt: &Stmt, var: &str, kind: EscapeKind) -> bool {
    match stmt {
        Stmt::Expression { expression } | Stmt::Say { expression, .. } => {
            expr_uses_var_safely(expression, var, ExprCtx::Data, kind)
        }
        Stmt::Return { value } => value
            .as_ref()
            .is_none_or(|v| expr_uses_var_safely(v, var, ExprCtx::Data, kind)),
        Stmt::Throw { value } => expr_uses_var_safely(value, var, ExprCtx::Data, kind),
        Stmt::Var {
            initializer: Some(init),
            ..
        } => {
            if let Expr::Variable { name: src } = init
                && src.lexeme() == var
            {
                return true;
            }
            expr_uses_var_safely(init, var, ExprCtx::Data, kind)
        }
        Stmt::Var { .. }
        | Stmt::Break { .. }
        | Stmt::Continue { .. }
        | Stmt::Function { .. }
        | Stmt::AsyncFunction { .. }
        | Stmt::Class { .. }
        | Stmt::Use { .. }
        | Stmt::Enum { .. }
        | Stmt::TypeAlias { .. }
        | Stmt::Test { .. }
        | Stmt::Trait { .. }
        | Stmt::Impl { .. } => true,
        Stmt::Block { statements, .. } => statements
            .iter()
            .all(|s| stmt_uses_var_safely(s, var, kind)),
        Stmt::If {
            condition,
            then_branch,
            elif_branches,
            else_branch,
        } => {
            expr_uses_var_safely(condition, var, ExprCtx::Data, kind)
                && stmt_uses_var_safely(then_branch, var, kind)
                && elif_branches.iter().all(|e| {
                    expr_uses_var_safely(&e.condition, var, ExprCtx::Data, kind)
                        && stmt_uses_var_safely(&e.body, var, kind)
                })
                && else_branch
                    .as_deref()
                    .is_none_or(|s| stmt_uses_var_safely(s, var, kind))
        }
        Stmt::While { condition, body } => {
            expr_uses_var_safely(condition, var, ExprCtx::Data, kind)
                && stmt_uses_var_safely(body, var, kind)
        }
        Stmt::DoWhile { body, condition } => {
            stmt_uses_var_safely(body, var, kind)
                && expr_uses_var_safely(condition, var, ExprCtx::Data, kind)
        }
        Stmt::For {
            init,
            condition,
            update,
            body,
        } => {
            init.as_deref()
                .is_none_or(|s| stmt_uses_var_safely(s, var, kind))
                && condition
                    .as_ref()
                    .is_none_or(|e| expr_uses_var_safely(e, var, ExprCtx::Data, kind))
                && update
                    .as_ref()
                    .is_none_or(|e| expr_uses_var_safely(e, var, ExprCtx::Data, kind))
                && stmt_uses_var_safely(body, var, kind)
        }
        Stmt::ForIn { iterable, body, .. } => {
            expr_uses_var_safely(iterable, var, ExprCtx::Data, kind)
                && stmt_uses_var_safely(body, var, kind)
        }
        Stmt::ForAwait { producer, body, .. } => {
            expr_uses_var_safely(producer, var, ExprCtx::Data, kind)
                && stmt_uses_var_safely(body, var, kind)
        }
        Stmt::Match {
            expression,
            cases,
            default_case,
        } => {
            expr_uses_var_safely(expression, var, ExprCtx::Data, kind)
                && cases.iter().all(|c| {
                    expr_uses_var_safely(&c.value, var, ExprCtx::Data, kind)
                        && c.guard
                            .as_ref()
                            .is_none_or(|g| expr_uses_var_safely(g, var, ExprCtx::Data, kind))
                        && stmt_uses_var_safely(&c.body, var, kind)
                })
                && default_case
                    .as_deref()
                    .is_none_or(|s| stmt_uses_var_safely(s, var, kind))
        }
        Stmt::Try {
            try_block,
            catch_block,
            finally_block,
            ..
        } => {
            stmt_uses_var_safely(try_block, var, kind)
                && catch_block
                    .as_deref()
                    .is_none_or(|s| stmt_uses_var_safely(s, var, kind))
                && finally_block
                    .as_deref()
                    .is_none_or(|s| stmt_uses_var_safely(s, var, kind))
        }
        Stmt::Retry {
            count,
            body,
            catch_block,
            ..
        } => {
            expr_uses_var_safely(count, var, ExprCtx::Data, kind)
                && stmt_uses_var_safely(body, var, kind)
                && catch_block
                    .as_deref()
                    .is_none_or(|s| stmt_uses_var_safely(s, var, kind))
        }
        Stmt::Unsafe { body } => stmt_uses_var_safely(body, var, kind),
        Stmt::Quiet { body, .. } => stmt_uses_var_safely(body, var, kind),
        Stmt::Destructure { initializer, .. } => {
            expr_uses_var_safely(initializer, var, ExprCtx::Data, kind)
        }
        // Goroutines and channel iteration are not yet supported by codegen;
        // conservatively refuse to classify them as safe.
        Stmt::ChanRecvFor { .. } | Stmt::Go { .. } => false,
    }
}

/// Compute the set of local variables in `body` that are safe to
/// stack-allocate: `var x = ClassName()` constructions (zero-argument, no
/// `init`) plus `var y = x` aliases, where no use of the object leaks its
/// address. Rejected candidates are propagated along copy edges — the
/// aliases all refer to the same object, so if one escapes, the whole group
/// stays on the heap. Intentionally conservative: a rejected use says
/// nothing definitive, so the object keeps the heap path.
pub(crate) fn analyze_stack_allocatable(body: &[Stmt], module: &Module<'_>) -> HashSet<String> {
    let mut candidates: HashSet<String> = HashSet::new();
    loop {
        let before = candidates.len();
        collect_stack_candidates(body, &mut candidates, module);
        if candidates.len() == before {
            break;
        }
    }

    let mut rejected: HashSet<String> = candidates
        .iter()
        .filter(|var| {
            !body
                .iter()
                .all(|s| stmt_uses_var_safely(s, var, EscapeKind::StackSlot))
        })
        .cloned()
        .collect();
    let mut copy_edges: Vec<(String, String)> = Vec::new();
    collect_copy_edges(body, &mut copy_edges);
    let mut changed = true;
    while changed {
        changed = false;
        for (src, dst) in &copy_edges {
            let src_rejected = rejected.contains(src);
            let dst_rejected = rejected.contains(dst);
            if (src_rejected && candidates.contains(dst) && !dst_rejected)
                || (dst_rejected && candidates.contains(src) && !src_rejected)
            {
                rejected.insert(src.clone());
                rejected.insert(dst.clone());
                changed = true;
            }
        }
    }

    candidates.difference(&rejected).cloned().collect()
}

/// Compute the set of local variables whose class instance may have its
/// owned fields reclaimed at scope exit. A candidate is a `var x =
/// ClassName(...)` construction (any constructor arity) or `var x =
/// copy(...)`; it is rejected when any use escapes the function or aliases
/// the instance. Escaping or aliased objects are leaked (never freed) rather
/// than freed twice; only provably non-aliased instances get their fields
/// dropped.
pub(crate) fn analyze_class_drops(body: &[Stmt], module: &Module<'_>) -> HashSet<String> {
    let mut candidates: HashSet<String> = HashSet::new();
    collect_class_drop_candidates(body, &mut candidates, module);

    let mut rejected: HashSet<String> = candidates
        .iter()
        .filter(|var| {
            !body
                .iter()
                .all(|s| stmt_uses_var_safely(s, var, EscapeKind::FieldDrop))
        })
        .cloned()
        .collect();

    let mut copy_edges: Vec<(String, String)> = Vec::new();
    collect_copy_edges(body, &mut copy_edges);
    for (src, dst) in &copy_edges {
        if candidates.contains(src) {
            rejected.insert(src.clone());
        }
        if candidates.contains(dst) {
            rejected.insert(dst.clone());
        }
    }

    candidates.difference(&rejected).cloned().collect()
}

/// Collect every `var x = ClassName(...)` construction (any constructor
/// arity) and `var x = copy(...)`, including declarations nested in blocks,
/// branches, loops, and handlers: an instance built inside a `try` or a loop
/// body owns its fields exactly like one built at the top level, and
/// skipping those leaked them. The rejection pass walks the whole body, so a
/// nested candidate is screened for escapes and aliasing on the same terms.
pub(crate) fn collect_class_drop_candidates(
    body: &[Stmt],
    candidates: &mut HashSet<String>,
    module: &Module<'_>,
) {
    for stmt in body {
        if let Stmt::Var {
            name,
            initializer,
            is_static,
            view,
            ..
        } = stmt
        {
            if *is_static || view.is_some() {
                continue;
            }
            let eligible = matches!(
                initializer,
                Some(Expr::Call { callee, .. })
                    if matches!(
                        callee.as_ref(),
                        Expr::Variable { name: ctor }
                            if module.get_struct_type(ctor.lexeme()).is_some()
                    )
            ) || matches!(initializer, Some(Expr::Copy { .. }))
                || matches!(
                    initializer,
                    Some(Expr::StructLiteral { class_name, .. })
                        if module.get_struct_type(class_name.lexeme()).is_some()
                );
            if eligible {
                candidates.insert(name.lexeme().to_string());
            }
        }
        match stmt {
            Stmt::Block { statements, .. } => {
                collect_class_drop_candidates(statements, candidates, module);
            }
            Stmt::If {
                then_branch,
                elif_branches,
                else_branch,
                ..
            } => {
                collect_class_drop_candidates(
                    std::slice::from_ref(then_branch),
                    candidates,
                    module,
                );
                for e in elif_branches {
                    collect_class_drop_candidates(
                        std::slice::from_ref(&e.body),
                        candidates,
                        module,
                    );
                }
                if let Some(else_body) = else_branch {
                    collect_class_drop_candidates(
                        std::slice::from_ref(else_body),
                        candidates,
                        module,
                    );
                }
            }
            Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => {
                collect_class_drop_candidates(std::slice::from_ref(body), candidates, module);
            }
            Stmt::For { init, body, .. } => {
                if let Some(init_stmt) = init {
                    collect_class_drop_candidates(
                        std::slice::from_ref(init_stmt),
                        candidates,
                        module,
                    );
                }
                collect_class_drop_candidates(std::slice::from_ref(body), candidates, module);
            }
            Stmt::ForIn { body, .. } | Stmt::ForAwait { body, .. } => {
                collect_class_drop_candidates(std::slice::from_ref(body), candidates, module);
            }
            Stmt::Match {
                cases,
                default_case,
                ..
            } => {
                for c in cases {
                    collect_class_drop_candidates(
                        std::slice::from_ref(&c.body),
                        candidates,
                        module,
                    );
                }
                if let Some(def) = default_case {
                    collect_class_drop_candidates(std::slice::from_ref(def), candidates, module);
                }
            }
            Stmt::Try {
                try_block,
                catch_block,
                finally_block,
                ..
            } => {
                collect_class_drop_candidates(std::slice::from_ref(try_block), candidates, module);
                if let Some(cb) = catch_block {
                    collect_class_drop_candidates(std::slice::from_ref(cb), candidates, module);
                }
                if let Some(fb) = finally_block {
                    collect_class_drop_candidates(std::slice::from_ref(fb), candidates, module);
                }
            }
            Stmt::Retry {
                body, catch_block, ..
            } => {
                collect_class_drop_candidates(std::slice::from_ref(body), candidates, module);
                if let Some(cb) = catch_block {
                    collect_class_drop_candidates(std::slice::from_ref(cb), candidates, module);
                }
            }
            Stmt::Unsafe { body } => {
                collect_class_drop_candidates(std::slice::from_ref(body), candidates, module);
            }
            Stmt::Quiet { body, .. } => {
                collect_class_drop_candidates(std::slice::from_ref(body), candidates, module);
            }
            _ => {}
        }
    }
}

/// Collect every `var dst = src` statement (copy edges) in `stmts`.
pub(crate) fn collect_copy_edges(stmts: &[Stmt], edges: &mut Vec<(String, String)>) {
    for stmt in stmts {
        if let Stmt::Var {
            name,
            initializer: Some(Expr::Variable { name: src }),
            ..
        } = stmt
        {
            edges.push((src.lexeme().to_string(), name.lexeme().to_string()));
        }
        match stmt {
            Stmt::Block { statements, .. } => collect_copy_edges(statements, edges),
            Stmt::If {
                then_branch,
                elif_branches,
                else_branch,
                ..
            } => {
                collect_copy_edges(std::slice::from_ref(then_branch), edges);
                for e in elif_branches {
                    collect_copy_edges(std::slice::from_ref(&e.body), edges);
                }
                if let Some(else_body) = else_branch {
                    collect_copy_edges(std::slice::from_ref(else_body), edges);
                }
            }
            Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => {
                collect_copy_edges(std::slice::from_ref(body), edges)
            }
            Stmt::For { init, body, .. } => {
                if let Some(init_stmt) = init {
                    collect_copy_edges(std::slice::from_ref(init_stmt), edges);
                }
                collect_copy_edges(std::slice::from_ref(body), edges);
            }
            Stmt::ForIn { body, .. } | Stmt::ForAwait { body, .. } => {
                collect_copy_edges(std::slice::from_ref(body), edges)
            }
            Stmt::Match {
                cases,
                default_case,
                ..
            } => {
                for c in cases {
                    collect_copy_edges(std::slice::from_ref(&c.body), edges);
                }
                if let Some(def) = default_case {
                    collect_copy_edges(std::slice::from_ref(def), edges);
                }
            }
            Stmt::Try {
                try_block,
                catch_block,
                finally_block,
                ..
            } => {
                collect_copy_edges(std::slice::from_ref(try_block), edges);
                if let Some(cb) = catch_block {
                    collect_copy_edges(std::slice::from_ref(cb), edges);
                }
                if let Some(fb) = finally_block {
                    collect_copy_edges(std::slice::from_ref(fb), edges);
                }
            }
            Stmt::Retry {
                body, catch_block, ..
            } => {
                collect_copy_edges(std::slice::from_ref(body), edges);
                if let Some(cb) = catch_block {
                    collect_copy_edges(std::slice::from_ref(cb), edges);
                }
            }
            Stmt::Unsafe { body } => collect_copy_edges(std::slice::from_ref(body), edges),
            Stmt::Quiet { body, .. } => collect_copy_edges(std::slice::from_ref(body), edges),
            _ => {}
        }
    }
}
