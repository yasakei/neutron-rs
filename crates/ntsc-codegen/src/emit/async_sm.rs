//! Lowering of `async fun` to poll-based state machines.
//!
//! Each async function becomes an opaque future struct (`ntsc_future_<name>`)
//! holding its `i32 state`, params, result, top-level locals, and one
//! sub-future slot per `await`, plus a poll function
//! (`i8 ntsc_future_<name>_poll(i8* future)`) that `switch`es on the state
//! field and returns 1 when done. All mutable state lives in the struct so it
//! survives suspension. Awaited callees are emitted first (reverse
//! topological order) so sub-future fields can reference their struct types;
//! cyclic `await` chains are compile errors. See docs/async-rfc.md §8.

use super::*;

// ── Async state machines ────────────────────────────────────────────────

/// The LLVM field type of an async future struct slot.
pub(crate) enum AsyncFieldTy {
    /// A regular NTSC-typed slot (state, parameter, result, or local).
    Native(Ty),

    /// A sub-future slot holding the child's future struct (one per await).
    Future(String),
}

/// One `await` point in an async body.
///
/// `stmt_idx` is the index of the awaiting statement in the flattened
/// top-level body. On resume, the child's result is loaded from
/// `child_result_index` (always `1 + <child param count>`: slot 0 is the
/// child's state, slots 1..=n its params, and its result sits right after)
/// and coerced to `child_ret_ty`.
pub(crate) struct AwaitInfo {
    stmt_idx: usize,
    child_name: String,
    child_ret_ty: Ty,
    child_result_index: u32,
}

/// The kind of a channel suspension point (a channel send or receive that
/// parks the goroutine like an `await`).
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum ChanOpKind {
    Send,
    Recv,
}

/// A single suspension point in the unified state ordering, indexed by its
/// position (segment ordinal). The await/chan indices reference the
/// `await_infos`/`chan_infos` lists.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum SuspKind {
    Await(usize),
    Chan(usize),
}

/// One channel send/receive suspension point in an async body.
///
/// Like `await`, channel ops are recognised only at the top level (statement
/// boundary, variable initializer, or return value). They park the running
/// goroutine: `ntask_chan_send`/`ntask_chan_recv` set the goroutine's park
/// state and the poll returns 0; on resume the goroutine's channel op has
/// been completed by the scheduler. A receive reads its result through
/// `ntask_chan_recv_result`.
pub(crate) struct ChanInfo {
    stmt_idx: usize,
    op: ChanOpKind,
    /// Coercion target type for a receive (`Ty::Void` for a send).
    recv_ty: Ty,
}

/// Pre-analyzed layout of a single async function's future struct.
///
/// Field order is fixed and ABI-relevant: `state` (0) | params | `result`
/// | locals | `sub_<child>` (one per await). The `fields` map keys params
/// and locals by their source names; `state`, `result`, and the `sub_*`
/// slots are addressed by the indices in `result_index` and
/// `sub_field_base`.
pub(crate) struct AsyncLayout {
    name: String,
    field_tys: Vec<AsyncFieldTy>,
    fields: HashMap<String, u32>,
    result_index: u32,

    /// Index of the first sub-future slot (after params, result, and locals).
    sub_field_base: u32,
    await_infos: Vec<AwaitInfo>,
    /// Channel send/receive suspension points, in top-level statement order.
    chan_infos: Vec<ChanInfo>,
    /// The suspend-state index (segment ordinal) for each `await_infos` entry.
    /// These and the channel points partition the state range
    /// `1..=suspension_count` in statement order; the segment splitter and
    /// the poll switch both key off this unified ordering.
    await_state_index: Vec<u32>,
    /// The suspend-state index (segment ordinal) for each `chan_infos` entry.
    chan_state_index: Vec<u32>,
    /// Total suspension points (awaits + channel ops); the done state is
    /// `suspension_count + 1`, and there are `suspension_count + 1` segments.
    suspension_count: u32,
    /// The suspension points in statement order; position i is segment i's
    /// resume dispatch and carries state index i.
    susp_order: Vec<SuspKind>,
    ret_ty: Ty,

    /// Anonymous async blocks compiled as part of this layout. Maps the
    /// generated function name to the block body.
    pub(crate) anon_async_blocks: Vec<(
        String,
        Vec<Stmt>,
        Option<ntsc_ast::expr::ReturnTypeAnnotation>,
    )>,

    /// Maps `Expr::AsyncBlock` spans to their generated anonymous function
    /// names, used during poll emission to resolve standalone blocks and
    /// `wait_any`/`wait_all` arguments.
    pub(crate) block_span_to_name: HashMap<usize, String>,
}

/// Infer the type of an async local that has no explicit annotation.
///
/// Type checking guarantees such a local is initialized with a literal or an
/// `await`, so the slot type is derived from those.
pub(crate) fn async_local_ty(
    program: &Program,
    type_annotation: &Option<ntsc_ast::types::TypeAnnotation>,
    initializer: &Option<Expr>,
) -> Ty {
    if let Some(annotation) = type_annotation {
        return type_annotation_to_ty(&Some(annotation.clone()));
    }
    if let Some(init) = initializer {
        if let Some(ty) = expr_to_literal_ty(init) {
            return ty;
        }
        if let Expr::Await { callee, .. } = init {
            let callee_name = match callee.as_ref() {
                Expr::Variable { name } => name.lexeme(),
                Expr::Member { object, property } if matches!(object.as_ref(), Expr::Variable { name } if name.lexeme() == "async") => {
                    property.lexeme()
                }
                Expr::AsyncBlock { return_type, .. } => {
                    return function_return_ty(return_type);
                }
                _ => return Ty::Void,
            };
            if callee_name == "sleep" {
                return Ty::Void;
            }
            for stmt in &program.statements {
                if let Stmt::AsyncFunction {
                    name: fn_name,
                    return_type,
                    ..
                } = stmt
                    && fn_name.lexeme() == callee_name
                {
                    return function_return_ty(return_type);
                }
            }
        }
    }
    Ty::Void
}

/// Collect every local variable of an async body into the future struct's
/// field map. Nested function/lambda bodies are independent functions (their
/// locals belong to their own futures) and are not descended into.
pub(crate) fn collect_async_locals(
    stmt: &Stmt,
    program: &Program,
    fields: &mut HashMap<String, u32>,
    field_names: &mut Vec<String>,
    field_tys: &mut Vec<AsyncFieldTy>,
) {
    let mut add = |name: &str, ty: Ty, fields: &mut HashMap<String, u32>| {
        if !fields.contains_key(name) {
            fields.insert(name.to_string(), field_names.len() as u32);
            field_names.push(name.to_string());
            field_tys.push(AsyncFieldTy::Native(ty));
        }
    };
    match stmt {
        Stmt::Var {
            name,
            type_annotation,
            initializer,
            ..
        } => {
            let ty = async_local_ty(program, type_annotation, initializer);
            add(name.lexeme(), ty, fields);
        }
        Stmt::Destructure { names, .. } => {
            for name in names {
                add(name.lexeme(), Ty::Any, fields);
            }
        }
        Stmt::ForIn { variable, body, .. } | Stmt::ForAwait { variable, body, .. } => {
            add(variable.lexeme(), Ty::Any, fields);
            collect_async_locals(body, program, fields, field_names, field_tys);
        }
        Stmt::Block { statements, .. } => {
            for inner in statements {
                collect_async_locals(inner, program, fields, field_names, field_tys);
            }
        }
        Stmt::If {
            then_branch,
            elif_branches,
            else_branch,
            ..
        } => {
            collect_async_locals(then_branch, program, fields, field_names, field_tys);
            for branch in elif_branches {
                collect_async_locals(&branch.body, program, fields, field_names, field_tys);
            }
            if let Some(else_branch) = else_branch {
                collect_async_locals(else_branch, program, fields, field_names, field_tys);
            }
        }
        Stmt::While { body, .. }
        | Stmt::DoWhile { body, .. }
        | Stmt::Retry { body, .. }
        | Stmt::Unsafe { body, .. }
        | Stmt::Quiet { body, .. } => {
            collect_async_locals(body, program, fields, field_names, field_tys)
        }
        Stmt::For { init, body, .. } => {
            if let Some(init) = init {
                collect_async_locals(init, program, fields, field_names, field_tys);
            }
            collect_async_locals(body, program, fields, field_names, field_tys);
        }
        Stmt::Match {
            cases,
            default_case,
            ..
        } => {
            for case in cases {
                collect_async_locals(&case.body, program, fields, field_names, field_tys);
            }
            if let Some(default_case) = default_case {
                collect_async_locals(default_case, program, fields, field_names, field_tys);
            }
        }
        Stmt::Try {
            try_block,
            catch_block,
            finally_block,
            ..
        } => {
            collect_async_locals(try_block, program, fields, field_names, field_tys);
            if let Some(catch_block) = catch_block {
                collect_async_locals(catch_block, program, fields, field_names, field_tys);
            }
            if let Some(finally_block) = finally_block {
                collect_async_locals(finally_block, program, fields, field_names, field_tys);
            }
        }
        Stmt::Function { .. } | Stmt::AsyncFunction { .. } => {}
        Stmt::Expression {
            expression: Expr::ChanRecv { receiver, .. },
            ..
        } => {
            // A `v |> ch` receive binds its receiver as a fresh local; type it
            // as `Any` for the field slot (receivers hold ints or owned
            // handles, both i64), with the precise element type supplied to
            // `define_var` on resume from the typeck registry.
            add(receiver.lexeme(), Ty::Any, fields);
        }
        _ => {}
    }
}

/// Flatten top-level `{ ... }` blocks of an async body into the statement
/// list. Type checking treats such blocks as transparent (awaits inside them
/// are legal), so the segment machinery must see their statements at the
/// top level. Nested blocks inside control flow are left untouched.
pub(crate) fn flatten_top_level_blocks(stmts: &[Stmt]) -> Vec<Stmt> {
    let mut out = Vec::new();
    for stmt in stmts {
        match stmt {
            Stmt::Block { statements, .. } => {
                out.extend(flatten_top_level_blocks(statements));
            }
            _ => out.push(stmt.clone()),
        }
    }
    out
}

pub(crate) fn build_async_layout(
    program: &Program,
    name: &ntsc_ast::token::Token,
    params: &[ntsc_ast::expr::FunctionParam],
    return_type: &Option<ntsc_ast::types::ReturnType>,
    body: &[Stmt],
) -> Result<AsyncLayout, crate::CodegenError> {
    let ret_ty = function_return_ty(return_type);

    let mut field_names = Vec::new();
    let mut field_tys = Vec::new();
    let mut fields = HashMap::new();

    fields.insert("state".to_string(), 0);
    field_names.push("state".to_string());
    field_tys.push(AsyncFieldTy::Native(Ty::Int));

    // Parameter slots (indices 1..=params.len()).
    for param in params {
        let key = param.name.lexeme().to_string();
        let param_ty = type_annotation_to_ty(&param.type_annotation);
        fields.insert(key.clone(), field_names.len() as u32);
        field_names.push(key.clone());
        field_tys.push(AsyncFieldTy::Native(param_ty));
    }

    let result_index = field_names.len() as u32;
    // Result slot, then local slots (declaration/discovery order).
    fields.insert("result".to_string(), result_index);
    field_names.push("result".to_string());
    field_tys.push(AsyncFieldTy::Native(ret_ty.clone()));

    for stmt in body {
        collect_async_locals(stmt, program, &mut fields, &mut field_names, &mut field_tys);
    }

    let sub_field_base = field_names.len() as u32;

    let mut await_infos = Vec::new();
    let mut await_state_index = Vec::new();
    let mut chan_infos = Vec::new();
    let mut anon_async_blocks = Vec::new();
    let mut anon_counter = 0usize;
    // Every suspension point (await or channel op) gets a distinct state index
    // equal to its 1-based ordinal in top-level statement order, shared by the
    // poll switch, the segment splitter, and the suspend/resume logic.
    let mut suspension_points = 0u32;
    let mut recv_index = 0usize;
    for (stmt_idx, stmt) in body.iter().enumerate() {
        let is_await_statement = matches!(
            stmt,
            Stmt::Expression {
                expression: Expr::Await { .. },
                ..
            } | Stmt::Var {
                initializer: Some(Expr::Await { .. }),
                ..
            } | Stmt::Return {
                value: Some(Expr::Await { .. }),
                ..
            }
        );
        if is_await_statement {
            let (child_name, child_ret_ty, anon_body, anon_ret) =
                await_callee_info(stmt, program, &mut anon_counter)?;
            if let Some(body) = anon_body {
                anon_async_blocks.push((child_name.clone(), body, anon_ret));
            }
            let child_param_count =
                await_callee_param_count(program, &child_name, &anon_async_blocks)?;
            // One sub-future slot per top-level await statement, plus its
            // resume metadata. Type checking guarantees awaits appear only
            // as statement-level calls, variable initializers, or return
            // values.
            field_names.push(format!("sub_{child_name}"));

            // `async.sleep` futures live in the runtime registry behind an
            // i64 handle, so its slot is a plain integer; awaited user
            // functions embed their child future struct inline.
            if child_name == "sleep" {
                field_tys.push(AsyncFieldTy::Native(Ty::Int));
            } else {
                field_tys.push(AsyncFieldTy::Future(child_name.clone()));
            }
            await_state_index.push(suspension_points);
            suspension_points += 1;
            await_infos.push(AwaitInfo {
                stmt_idx,
                child_name,
                child_ret_ty,
                child_result_index: 1 + child_param_count as u32,
            });
        } else if let Some((op, recv_ty)) =
            chan_target_ty(stmt, &ret_ty, name.lexeme(), &mut recv_index)
        {
            chan_infos.push(ChanInfo {
                stmt_idx,
                op,
                recv_ty,
            });
            suspension_points += 1;
        }
    }

    // Discover standalone Expr::AsyncBlock (not inside await) from
    // wait_any/wait_all arguments and expression-position blocks.
    let mut block_span_to_name = HashMap::new();
    collect_standalone_async_blocks(
        body,
        &mut anon_async_blocks,
        &mut anon_counter,
        &mut block_span_to_name,
    );

    // Merge the await and channel suspension points into a single ordered
    // list (both are already sorted ascending by `stmt_idx`). Position i is
    // the resume/suspend dispatch for segment i and carries state index i.
    let mut susp_order = Vec::with_capacity(suspension_points as usize);
    let mut chan_state_index = Vec::with_capacity(chan_infos.len());
    let (mut ai, mut ci) = (0usize, 0usize);
    while ai < await_infos.len() || ci < chan_infos.len() {
        let pick_await = match (await_infos.get(ai), chan_infos.get(ci)) {
            (Some(a), Some(c)) => a.stmt_idx <= c.stmt_idx,
            (Some(_), None) => true,
            (None, Some(_)) => false,
            (None, None) => break,
        };
        if pick_await {
            susp_order.push(SuspKind::Await(ai));
            ai += 1;
        } else {
            susp_order.push(SuspKind::Chan(ci));
            chan_state_index.push(susp_order.len() as u32);
            ci += 1;
        }
    }

    Ok(AsyncLayout {
        name: name.lexeme().to_string(),
        field_tys,
        fields,
        result_index,
        sub_field_base,
        await_infos,
        chan_infos,
        await_state_index,
        chan_state_index,
        suspension_count: suspension_points,
        susp_order,
        ret_ty,
        anon_async_blocks,
        block_span_to_name,
    })
}

/// `(child_name, return_type, anon_body?, anon_return_type?)` returned by
/// `await_callee_info`.
pub(crate) type AwaitCalleeResult = (
    String,
    Ty,
    Option<Vec<Stmt>>,
    Option<ntsc_ast::expr::ReturnTypeAnnotation>,
);

/// Scan statements for `Expr::AsyncBlock` that appear outside of `await`
/// (standalone expression blocks and `wait_any`/`wait_all` arguments) and
/// register them as anonymous async blocks for compilation.
fn collect_standalone_async_blocks(
    stmts: &[Stmt],
    anon_async_blocks: &mut Vec<(
        String,
        Vec<Stmt>,
        Option<ntsc_ast::expr::ReturnTypeAnnotation>,
    )>,
    anon_counter: &mut usize,
    block_span_to_name: &mut HashMap<usize, String>,
) {
    for stmt in stmts {
        match stmt {
            Stmt::Expression { expression, .. } => {
                collect_expr_async_blocks(
                    expression,
                    anon_async_blocks,
                    anon_counter,
                    block_span_to_name,
                );
            }
            Stmt::Var {
                initializer: Some(init),
                ..
            } => {
                collect_expr_async_blocks(
                    init,
                    anon_async_blocks,
                    anon_counter,
                    block_span_to_name,
                );
            }
            Stmt::Return {
                value: Some(val), ..
            } => {
                collect_expr_async_blocks(val, anon_async_blocks, anon_counter, block_span_to_name);
            }
            Stmt::Block { statements, .. } => {
                collect_standalone_async_blocks(
                    statements,
                    anon_async_blocks,
                    anon_counter,
                    block_span_to_name,
                );
            }
            Stmt::If {
                condition,
                then_branch,
                elif_branches: _,
                else_branch,
            } => {
                collect_expr_async_blocks(
                    condition,
                    anon_async_blocks,
                    anon_counter,
                    block_span_to_name,
                );
                collect_standalone_async_blocks(
                    std::slice::from_ref(then_branch),
                    anon_async_blocks,
                    anon_counter,
                    block_span_to_name,
                );
                if let Some(eb) = else_branch {
                    collect_standalone_async_blocks(
                        std::slice::from_ref(eb),
                        anon_async_blocks,
                        anon_counter,
                        block_span_to_name,
                    );
                }
            }
            Stmt::While {
                condition, body, ..
            } => {
                collect_expr_async_blocks(
                    condition,
                    anon_async_blocks,
                    anon_counter,
                    block_span_to_name,
                );
                collect_standalone_async_blocks(
                    std::slice::from_ref(body),
                    anon_async_blocks,
                    anon_counter,
                    block_span_to_name,
                );
            }
            Stmt::For {
                init,
                condition,
                update,
                body,
                ..
            } => {
                if let Some(i) = init {
                    collect_standalone_async_blocks(
                        std::slice::from_ref(i),
                        anon_async_blocks,
                        anon_counter,
                        block_span_to_name,
                    );
                }
                if let Some(c) = condition {
                    collect_expr_async_blocks(
                        c,
                        anon_async_blocks,
                        anon_counter,
                        block_span_to_name,
                    );
                }
                if let Some(u) = update {
                    collect_expr_async_blocks(
                        u,
                        anon_async_blocks,
                        anon_counter,
                        block_span_to_name,
                    );
                }
                collect_standalone_async_blocks(
                    std::slice::from_ref(body),
                    anon_async_blocks,
                    anon_counter,
                    block_span_to_name,
                );
            }
            Stmt::ForIn { body, .. } | Stmt::ForAwait { body, .. } => {
                collect_standalone_async_blocks(
                    std::slice::from_ref(body),
                    anon_async_blocks,
                    anon_counter,
                    block_span_to_name,
                );
            }
            Stmt::Try {
                try_block,
                catch_block,
                finally_block,
                ..
            } => {
                collect_standalone_async_blocks(
                    std::slice::from_ref(try_block),
                    anon_async_blocks,
                    anon_counter,
                    block_span_to_name,
                );
                if let Some(cb) = catch_block {
                    collect_standalone_async_blocks(
                        std::slice::from_ref(cb),
                        anon_async_blocks,
                        anon_counter,
                        block_span_to_name,
                    );
                }
                if let Some(fb) = finally_block {
                    collect_standalone_async_blocks(
                        std::slice::from_ref(fb),
                        anon_async_blocks,
                        anon_counter,
                        block_span_to_name,
                    );
                }
            }
            _ => {}
        }
    }
}

fn collect_expr_async_blocks(
    expr: &Expr,
    anon_async_blocks: &mut Vec<(
        String,
        Vec<Stmt>,
        Option<ntsc_ast::expr::ReturnTypeAnnotation>,
    )>,
    anon_counter: &mut usize,
    block_span_to_name: &mut HashMap<usize, String>,
) {
    match expr {
        Expr::AsyncBlock {
            body,
            return_type,
            span,
            ..
        } => {
            *anon_counter += 1;
            let name = format!("__anon_async_{}", anon_counter);
            anon_async_blocks.push((name.clone(), body.clone(), return_type.clone()));
            block_span_to_name.insert(span.start, name);
        }
        Expr::Call {
            callee, arguments, ..
        } => {
            if is_wait_any_or_all(callee) {
                for arg in arguments {
                    collect_expr_async_blocks(
                        arg,
                        anon_async_blocks,
                        anon_counter,
                        block_span_to_name,
                    );
                }
            }
        }
        Expr::Await {
            callee, arguments, ..
        } => {
            collect_expr_async_blocks(callee, anon_async_blocks, anon_counter, block_span_to_name);
            for arg in arguments {
                collect_expr_async_blocks(arg, anon_async_blocks, anon_counter, block_span_to_name);
            }
        }
        _ => {}
    }
}

fn is_wait_any_or_all(expr: &Expr) -> bool {
    matches!(expr, Expr::Variable { name } if name.lexeme() == "wait_any" || name.lexeme() == "wait_all")
}

pub(crate) fn await_callee_info(
    stmt: &Stmt,
    program: &Program,
    anon_counter: &mut usize,
) -> Result<AwaitCalleeResult, crate::CodegenError> {
    let (callee_expr, _arguments) = await_stmt_parts(stmt)?;
    match callee_expr {
        Expr::Variable { name } => {
            let callee_name = name.lexeme();
            if callee_name == "sleep" {
                return Ok(("sleep".to_string(), Ty::Void, None, None));
            }
            let ret_ty = program
                .statements
                .iter()
                .find_map(|s| match s {
                    Stmt::AsyncFunction {
                        name: fn_name,
                        return_type,
                        ..
                    } if fn_name.lexeme() == callee_name => Some(function_return_ty(return_type)),
                    _ => None,
                })
                .ok_or_else(|| {
                    crate::CodegenError::LLVMError(format!(
                        "internal: awaited callee `{callee_name}` is not a module-level async function"
                    ))
                })?;
            Ok((callee_name.to_string(), ret_ty, None, None))
        }
        Expr::Member { object, property }
            if matches!(
                object.as_ref(),
                Expr::Variable { name } if name.lexeme() == "async"
            ) =>
        {
            let callee_name = property.lexeme();
            if callee_name == "sleep" {
                return Ok(("sleep".to_string(), Ty::Void, None, None));
            }
            let ret_ty = program
                .statements
                .iter()
                .find_map(|s| match s {
                    Stmt::AsyncFunction {
                        name: fn_name,
                        return_type,
                        ..
                    } if fn_name.lexeme() == callee_name => Some(function_return_ty(return_type)),
                    _ => None,
                })
                .ok_or_else(|| {
                    crate::CodegenError::LLVMError(format!(
                        "internal: awaited callee `{callee_name}` is not a module-level async function"
                    ))
                })?;
            Ok((callee_name.to_string(), ret_ty, None, None))
        }
        Expr::AsyncBlock {
            body, return_type, ..
        } => {
            *anon_counter += 1;
            let name = format!("__anon_async_{}", anon_counter);
            let ret_ty = function_return_ty(return_type);
            Ok((name, ret_ty, Some(body.clone()), return_type.clone()))
        }
        _ => Err(crate::CodegenError::LLVMError(
            "await requires a call to a module-level async function".into(),
        )),
    }
}

pub(crate) fn await_callee_param_count(
    program: &Program,
    child_name: &str,
    anon_async_blocks: &[(
        String,
        Vec<Stmt>,
        Option<ntsc_ast::expr::ReturnTypeAnnotation>,
    )],
) -> Result<usize, crate::CodegenError> {
    if child_name == "sleep" {
        return Ok(1);
    }
    // Anonymous async blocks have no params.
    if anon_async_blocks
        .iter()
        .any(|(name, _, _)| name == child_name)
    {
        return Ok(0);
    }
    program
        .statements
        .iter()
        .find_map(|s| match s {
            Stmt::AsyncFunction { name, params, .. } if name.lexeme() == child_name => {
                Some(params.len())
            }
            _ => None,
        })
        .ok_or_else(|| {
            crate::CodegenError::LLVMError(format!(
                "internal: awaited callee `{child_name}` not found"
            ))
        })
}

/// The (callee, arguments) of an await statement, for all three legal
/// statement shapes (expression, variable initializer, return value).
pub(crate) fn await_stmt_parts(stmt: &Stmt) -> Result<(&Expr, &[Expr]), crate::CodegenError> {
    match stmt {
        Stmt::Expression {
            expression: Expr::Await {
                callee, arguments, ..
            },
            ..
        }
        | Stmt::Var {
            initializer: Some(Expr::Await {
                callee, arguments, ..
            }),
            ..
        }
        | Stmt::Return {
            value: Some(Expr::Await {
                callee, arguments, ..
            }),
            ..
        } => Ok((callee.as_ref(), arguments)),
        _ => Err(crate::CodegenError::LLVMError(
            "internal: expected an await statement".into(),
        )),
    }
}

/// The channel-op kind of a top-level channel suspension statement. Sends
/// (`ch <| v`) and receives (`v |> ch`) both appear as top-level statement
/// expressions; the receive's freshly-bound receiver variable is typed from
/// the element type typeck recorded for the owning function.
fn chan_target_ty(
    stmt: &Stmt,
    _ret_ty: &Ty,
    fn_name: &str,
    recv_index: &mut usize,
) -> Option<(ChanOpKind, Ty)> {
    match stmt {
        Stmt::Expression {
            expression: Expr::ChanSend { .. },
            ..
        } => Some((ChanOpKind::Send, Ty::Void)),
        Stmt::Expression {
            expression: Expr::ChanRecv { .. },
            ..
        } => {
            let element = ntsc_typeck::chan_receiver_element_types(fn_name)
                .get(*recv_index)
                .cloned()
                .unwrap_or(Ty::Any);
            *recv_index += 1;
            Some((ChanOpKind::Recv, element))
        }
        _ => None,
    }
}

/// The channel operand expression of a channel suspension statement.
pub(crate) fn chan_stmt_channel(stmt: &Stmt) -> Result<&Expr, crate::CodegenError> {
    match stmt {
        Stmt::Expression {
            expression: Expr::ChanSend { channel, .. },
            ..
        }
        | Stmt::Expression {
            expression: Expr::ChanRecv { channel, .. },
            ..
        } => Ok(channel),
        _ => Err(crate::CodegenError::LLVMError(
            "internal: expected a channel suspension statement".into(),
        )),
    }
}

/// The value expression sent by a channel-send suspension statement.
pub(crate) fn chan_stmt_value(stmt: &Stmt) -> Result<&Expr, crate::CodegenError> {
    match stmt {
        Stmt::Expression {
            expression: Expr::ChanSend { value, .. },
            ..
        } => Ok(value),
        _ => Err(crate::CodegenError::LLVMError(
            "internal: expected a channel-send statement".into(),
        )),
    }
}

/// The receiver variable of a channel-receive suspension statement.
pub(crate) fn chan_stmt_receiver(
    stmt: &Stmt,
) -> Result<&ntsc_ast::token::Token, crate::CodegenError> {
    match stmt {
        Stmt::Expression {
            expression: Expr::ChanRecv { receiver, .. },
            ..
        } => Ok(receiver),
        _ => Err(crate::CodegenError::LLVMError(
            "internal: expected a channel-receive statement".into(),
        )),
    }
}

/// Declare the (opaque) future struct type for an async function. Called
/// for callees before callers so sub-future fields can reference resolved
/// types.
pub(crate) fn declare_async_future<'ctx>(
    context: &'ctx Context,
    module: &Module<'ctx>,
    layout: &AsyncLayout,
) -> Result<inkwell::types::StructType<'ctx>, crate::CodegenError> {
    let struct_name = format!("ntsc_future_{}", layout.name);
    let struct_ty = context.opaque_struct_type(&struct_name);
    let field_types: Vec<inkwell::types::BasicTypeEnum<'ctx>> = layout
        .field_tys
        .iter()
        .map(|field| match field {
            AsyncFieldTy::Native(ty) => Ok(ty_to_llvm(ty, context)),
            AsyncFieldTy::Future(child) => module
                .get_struct_type(&format!("ntsc_future_{child}"))
                .ok_or_else(|| {
                    crate::CodegenError::LLVMError(format!(
                        "internal: child future `ntsc_future_{child}` not declared"
                    ))
                })
                .map(|t| t.as_basic_type_enum()),
        })
        .collect::<Result<Vec<_>, _>>()?;
    struct_ty.set_body(&field_types, false);
    Ok(struct_ty)
}

/// Emit the async state machine for one module-level async function.
/// Awaited callees are emitted first (reverse topological order); cycles are
/// rejected.
#[allow(clippy::too_many_arguments)]
pub(crate) fn emit_async_function<'ctx>(
    context: &'ctx Context,
    module: &Module<'ctx>,
    program: &Program,
    decl: &Stmt,
    done: &mut HashSet<String>,
    in_progress: &mut HashSet<String>,
) -> Result<(), crate::CodegenError> {
    let (name_token, params, return_type, body) = match decl {
        Stmt::AsyncFunction {
            name,
            params,
            return_type,
            body,
            ..
        } => (name, params, return_type, body),
        _ => return Ok(()),
    };
    let fn_name = name_token.lexeme();

    if done.contains(fn_name) {
        return Ok(());
    }
    if !in_progress.insert(fn_name.to_string()) {
        return Err(crate::CodegenError::LLVMError(format!(
            "cyclic await chain involving async function `{fn_name}` is not supported"
        )));
    }

    let body = flatten_top_level_blocks(body);

    let layout = build_async_layout(program, name_token, params, return_type, &body)?;

    // Emit anonymous async blocks first.
    for (anon_name, anon_body, anon_ret) in &layout.anon_async_blocks {
        let anon_name_token = ntsc_ast::token::Token::new(
            ntsc_ast::token::TokenKind::Identifier(anon_name.clone()),
            ntsc_ast::span::Span::dummy(),
        );
        let anon_decl = Stmt::AsyncFunction {
            name: anon_name_token,
            params: vec![],
            return_type: anon_ret.clone(),
            body: anon_body.clone(),
        };
        emit_async_function(context, module, program, &anon_decl, done, in_progress)?;
    }

    // Emit awaited callees first so their future struct types exist when
    // this future's fields reference them (reverse topological order). The
    // built-in `async.sleep` has no emitted callee: its future struct and
    // poll function are declared as part of the runtime.
    for info in &layout.await_infos {
        if info.child_name == "sleep" {
            continue;
        }
        // Anonymous async blocks were already emitted above.
        if layout
            .anon_async_blocks
            .iter()
            .any(|(n, _, _)| n == &info.child_name)
        {
            continue;
        }
        let callee = program
            .statements
            .iter()
            .find(|s| {
                matches!(s, Stmt::AsyncFunction { name, .. } if name.lexeme() == info.child_name)
            })
            .ok_or_else(|| {
                crate::CodegenError::LLVMError(format!(
                    "internal: awaited callee `{}` not found",
                    info.child_name
                ))
            })?;
        emit_async_function(context, module, program, callee, done, in_progress)?;
    }

    let struct_name = format!("ntsc_future_{fn_name}");
    declare_async_future(context, module, &layout)?;
    declare_async_drop(module, context, &struct_name)?;

    emit_async_poll(context, module, &struct_name, &layout, &body)?;
    emit_async_drop(context, module, &struct_name, &layout)?;

    if fn_name == "main" {
        emit_async_main_wrapper(context, module, &struct_name, &layout)?;
    }

    in_progress.remove(fn_name);
    done.insert(fn_name.to_string());
    Ok(())
}

/// Emit `ntsc_future_<name>_poll(i64 future) -> i8`, the state machine.
/// The poll `switch`es on the future's `state` field: 0 → segment 0 (the
/// statements before the first await), k in 1..=N → segment k (resumes
/// await k-1 first, then runs up to await k), N+1 → `finish` (done, stores
/// the default result for bodies that fall off the end, returns 1);
/// unknown states fall through to `finish` so the executor never loops
/// forever on a corrupt future.
pub(crate) fn emit_async_poll<'ctx>(
    context: &'ctx Context,
    module: &Module<'ctx>,
    struct_name: &str,
    layout: &AsyncLayout,
    body: &[Stmt],
) -> Result<(), crate::CodegenError> {
    let future_ty = module.get_struct_type(struct_name).ok_or_else(|| {
        crate::CodegenError::LLVMError(format!("internal: missing future struct {struct_name}"))
    })?;
    let poll_name = format!("ntsc_future_{}_poll", layout.name);
    let poll_fn = module.add_function(
        &poll_name,
        context
            .i8_type()
            .fn_type(&[context.i64_type().into()], false),
        Some(inkwell::module::Linkage::External),
    );

    let builder = context.create_builder();
    let entry_builder = context.create_builder();
    let entry_bb = context.append_basic_block(poll_fn, "entry");
    builder.position_at_end(entry_bb);
    entry_builder.position_at_end(entry_bb);

    let future_handle = poll_fn
        .get_nth_param(0)
        .ok_or_else(|| crate::CodegenError::LLVMError("missing poll future param".into()))?
        .into_int_value();
    // The poll ABI carries the future address as an i64 handle; recover the
    // pointer so the state machine can access the future struct.
    let future_ptr = builder.build_int_to_ptr(
        future_handle,
        context.ptr_type(AddressSpace::default()),
        "future_i8",
    )?;

    let future = builder.build_pointer_cast(
        future_ptr,
        context.ptr_type(AddressSpace::default()),
        "future",
    )?;
    let state_field = builder.build_struct_gep(future_ty, future, 0, "state_ptr")?;
    let state = builder.build_load(context.i32_type(), state_field, "state")?;

    let seg_count = (layout.suspension_count as usize) + 1;
    let finish_bb = context.append_basic_block(poll_fn, "finish");
    let fallthrough_bb = context.append_basic_block(poll_fn, "fallthrough");
    let seg_blocks: Vec<_> = (0..seg_count)
        .map(|k| context.append_basic_block(poll_fn, &format!("seg_{k}")))
        .collect();

    let mut switch_cases: Vec<(
        inkwell::values::IntValue<'ctx>,
        inkwell::basic_block::BasicBlock<'ctx>,
    )> = Vec::new();
    for (k, seg_bb) in seg_blocks.iter().enumerate() {
        switch_cases.push((context.i32_type().const_int(k as u64, false), *seg_bb));
    }

    switch_cases.push((
        context.i32_type().const_int(seg_count as u64, false),
        finish_bb,
    ));
    builder.build_switch(state.into_int_value(), fallthrough_bb, &switch_cases)?;

    for (k, seg_bb) in seg_blocks.iter().enumerate() {
        builder.position_at_end(*seg_bb);
        let mut fn_ctx = FunctionContext::new(
            poll_fn,
            &builder,
            &entry_builder,
            *seg_bb,
            module,
            Ty::Void,
            context,
        );
        fn_ctx.future_base = Some((future, future_ty));
        fn_ctx.async_fields = Some(layout.fields.clone());
        fn_ctx.block_span_to_name = Some(layout.block_span_to_name.clone());

        // Async state machines have no exception support: calls never check
        // the pending-exception flag. A segment can still *raise* (the
        // arithmetic guards throw on overflow), so the runtime lands the
        // exception on the executor, which aborts the program.
        fn_ctx.exception_checks = false;
        emit_async_segment(&mut fn_ctx, layout, body, k, &seg_blocks, finish_bb)?;

        if let Some(exc_bb) = fn_ctx.exception_return_bb {
            builder.position_at_end(exc_bb);
            builder.build_unconditional_branch(finish_bb)?;
        }
    }

    builder.position_at_end(fallthrough_bb);
    builder.build_return(Some(&context.i8_type().const_int(1, false)))?;

    builder.position_at_end(finish_bb);
    if layout.ret_ty != Ty::Void {
        let result_ptr =
            builder.build_struct_gep(future_ty, future, layout.result_index, "result_ptr")?;
        let default_val = default_llvm_value(&layout.ret_ty, context);
        builder.build_store(result_ptr, default_val)?;
    }
    builder.build_return(Some(&context.i8_type().const_int(1, false)))?;

    Ok(())
}

fn declare_async_drop<'ctx>(
    module: &Module<'ctx>,
    context: &'ctx Context,
    struct_name: &str,
) -> Result<(), crate::CodegenError> {
    let name = format!("{struct_name}_drop");
    if module.get_function(&name).is_none() {
        module.add_function(
            &name,
            context
                .void_type()
                .fn_type(&[context.ptr_type(AddressSpace::default()).into()], false),
            Some(inkwell::module::Linkage::Internal),
        );
    }
    Ok(())
}

fn emit_async_drop<'ctx>(
    context: &'ctx Context,
    module: &Module<'ctx>,
    struct_name: &str,
    layout: &AsyncLayout,
) -> Result<(), crate::CodegenError> {
    let function = module
        .get_function(&format!("{struct_name}_drop"))
        .ok_or_else(|| crate::CodegenError::LLVMError("missing async drop function".into()))?;
    let entry = context.append_basic_block(function, "entry");
    let builder = context.create_builder();
    let entry_builder = context.create_builder();
    builder.position_at_end(entry);
    entry_builder.position_at_end(entry);
    let future_ty = module.get_struct_type(struct_name).ok_or_else(|| {
        crate::CodegenError::LLVMError(format!("missing future struct {struct_name}"))
    })?;
    let future = function
        .get_nth_param(0)
        .ok_or_else(|| crate::CodegenError::LLVMError("missing async drop parameter".into()))?
        .into_pointer_value();
    let mut fn_ctx = FunctionContext::new(
        function,
        &builder,
        &entry_builder,
        entry,
        module,
        Ty::Void,
        context,
    );
    fn_ctx.future_base = Some((future, future_ty));

    for (index, field) in layout.field_tys.iter().enumerate() {
        let ptr = fn_ctx.future_field(index as u32)?;
        match field {
            AsyncFieldTy::Native(ty) if index != 0 && index as u32 != layout.result_index => {
                if ty_is_owned_handle(ty) {
                    let value = builder.build_load(ty_to_llvm(ty, context), ptr, "future_drop")?;
                    emit_drop_value(&mut fn_ctx, &TypedValue::new(value, ty.clone()))?;
                    builder.build_store(ptr, default_llvm_value(ty, context))?;
                }
            }
            AsyncFieldTy::Future(child) => {
                let child_fn = module
                    .get_function(&format!("ntsc_future_{child}_drop"))
                    .ok_or_else(|| {
                        crate::CodegenError::LLVMError(format!(
                            "missing child future drop function for `{child}`"
                        ))
                    })?;
                let child_ptr = builder.build_pointer_cast(
                    ptr,
                    context.ptr_type(AddressSpace::default()),
                    "child_future_drop",
                )?;
                builder.build_call(child_fn, &[child_ptr.into()], "child_drop")?;
            }
            _ => {}
        }
    }
    builder.build_return(None)?;
    Ok(())
}

/// Emit one segment of the state machine: rebind the future fields as
/// locals, resume the previous await if this is segment k>0, run the
/// segment's statements, then either suspend on the segment's await or
/// branch to `finish` when it is the last segment.
pub(crate) fn emit_async_segment<'ctx>(
    fn_ctx: &mut FunctionContext<'ctx, '_>,
    layout: &AsyncLayout,
    body: &[Stmt],
    segment_index: usize,
    seg_blocks: &[inkwell::basic_block::BasicBlock<'ctx>],
    finish_bb: inkwell::basic_block::BasicBlock<'ctx>,
) -> Result<(), crate::CodegenError> {
    let (start, end, resume) = if segment_index == 0 {
        let end = layout
            .susp_order
            .first()
            .map(|kind| susp_stmt_idx(layout, kind))
            .unwrap_or(body.len());
        (0, end, None)
    } else {
        let prev = layout.susp_order[segment_index - 1];
        let end = layout
            .susp_order
            .get(segment_index)
            .map(|kind| susp_stmt_idx(layout, kind))
            .unwrap_or(body.len());
        (susp_stmt_idx(layout, &prev) + 1, end, Some(prev))
    };

    for (name, index) in &layout.fields {
        if name == "state" || name == "result" {
            continue;
        }
        let ty = match &layout.field_tys[*index as usize] {
            AsyncFieldTy::Native(ty) => ty.clone(),
            AsyncFieldTy::Future(_) => continue,
        };
        let ptr = fn_ctx.future_field(*index)?;
        fn_ctx.define_var(name, ptr, ty);
    }

    if let Some(kind) = &resume {
        match kind {
            SuspKind::Await(await_idx) => {
                emit_await_resume(fn_ctx, layout, body, *await_idx)?;
            }
            SuspKind::Chan(chan_idx) => {
                emit_chan_resume(fn_ctx, layout, body, *chan_idx)?;
            }
        }
    }

    for stmt in &body[start..end] {
        if let Stmt::Return { value } = stmt {
            let stored = if let Some(expr) = value {
                let val = emit_expression(fn_ctx, expr)?;
                let coerced = coerce_value(fn_ctx, val, &layout.ret_ty)?;
                if let Expr::Variable { name } = expr
                    && ty_is_owned_handle(&coerced.ntsc_type)
                    && let Some((ptr, _)) = fn_ctx.variables.get(name.lexeme())
                {
                    fn_ctx.builder.build_store(
                        *ptr,
                        default_llvm_value(&coerced.ntsc_type, fn_ctx.context),
                    )?;
                }
                Some(coerced.value)
            } else {
                None
            };
            emit_async_return(fn_ctx, layout, stored)?;
        } else {
            emit_statement_in_function(fn_ctx, stmt)?;
        }
    }

    if let Some(kind) = layout.susp_order.get(segment_index) {
        match kind {
            SuspKind::Await(await_idx) => {
                emit_await_suspend(fn_ctx, layout, body, *await_idx)?;
            }
            SuspKind::Chan(chan_idx) => {
                emit_chan_suspend(fn_ctx, layout, body, *chan_idx)?;
            }
        }
    } else if fn_ctx
        .builder
        .get_insert_block()
        .map(|block| block.get_terminator().is_none())
        .unwrap_or(false)
    {
        fn_ctx.builder.build_unconditional_branch(finish_bb)?;
    }

    let _ = seg_blocks;
    Ok(())
}

/// The source statement index of a suspension point in the unified order.
fn susp_stmt_idx(layout: &AsyncLayout, kind: &SuspKind) -> usize {
    match kind {
        SuspKind::Await(i) => layout.await_infos[*i].stmt_idx,
        SuspKind::Chan(i) => layout.chan_infos[*i].stmt_idx,
    }
}

/// Suspend on await `await_idx`: zero the child's future slot, evaluate the
/// await arguments into the child's parameter fields, push the child poll
/// onto the executor, store resume state `k+1`, and return 0.
pub(crate) fn emit_await_suspend<'ctx>(
    fn_ctx: &mut FunctionContext<'ctx, '_>,
    layout: &AsyncLayout,
    body: &[Stmt],
    await_idx: usize,
) -> Result<(), crate::CodegenError> {
    let info = &layout.await_infos[await_idx];
    let stmt = &body[info.stmt_idx];
    let (_callee_expr, arguments) = await_stmt_parts(stmt)?;

    let child_slot = fn_ctx.future_field(layout.sub_field_base + await_idx as u32)?;

    if info.child_name == "sleep" {
        let sleep_new_fn = fn_ctx
            .module
            .get_function("ntsc_async_sleep_new")
            .ok_or_else(|| {
                crate::CodegenError::LLVMError("ntsc_async_sleep_new not declared".into())
            })?;
        let arg_values = emit_call_arguments(fn_ctx, arguments)?;
        let ms = arg_values
            .first()
            .ok_or_else(|| {
                crate::CodegenError::LLVMError("async.sleep requires a duration argument".into())
            })?
            .value
            .into_int_value();
        let sleep_result = fn_ctx
            .builder
            .build_call(sleep_new_fn, &[ms.into()], "sleep_new")?;
        let sleep_handle = call_result_to_value(fn_ctx, &sleep_result);
        fn_ctx.builder.build_store(child_slot, sleep_handle)?;

        let sleep_poll_fn = fn_ctx
            .module
            .get_function("ntsc_async_sleep_poll")
            .ok_or_else(|| {
                crate::CodegenError::LLVMError("ntsc_async_sleep_poll not declared".into())
            })?;
        let poll_ptr = sleep_poll_fn.as_global_value().as_pointer_value();
        let poll_i8 = fn_ctx.builder.build_pointer_cast(
            poll_ptr,
            fn_ctx.context.ptr_type(AddressSpace::default()),
            "sleep_poll_fn",
        )?;
        let push_fn = fn_ctx
            .module
            .get_function("ntsc_async_push")
            .ok_or_else(|| crate::CodegenError::LLVMError("ntsc_async_push not declared".into()))?;
        fn_ctx.builder.build_call(
            push_fn,
            &[poll_i8.into(), sleep_handle.into()],
            "async_push",
        )?;

        let state_ptr = fn_ctx.future_field(0)?;
        let next_state = fn_ctx
            .context
            .i32_type()
            .const_int((layout.await_state_index[await_idx] as u64) + 1, false);
        fn_ctx.builder.build_store(state_ptr, next_state)?;
        fn_ctx
            .builder
            .build_return(Some(&fn_ctx.context.i8_type().const_int(0, false)))?;
        return Ok(());
    }

    let child_struct_name = format!("ntsc_future_{}", info.child_name);
    let child_struct_ty = fn_ctx
        .module
        .get_struct_type(&child_struct_name)
        .ok_or_else(|| {
            crate::CodegenError::LLVMError(format!(
                "internal: child future {child_struct_name} not declared"
            ))
        })?;

    let child_ptr = fn_ctx.builder.build_pointer_cast(
        child_slot,
        fn_ctx.context.ptr_type(AddressSpace::default()),
        "child_future",
    )?;

    let zero = fn_ctx.context.i8_type().const_zero();
    let child_size = child_struct_ty.size_of().ok_or_else(|| {
        crate::CodegenError::LLVMError(format!("internal: {child_struct_name} has no size"))
    })?;
    fn_ctx
        .builder
        .build_memset(child_ptr, 1, zero, child_size)?;

    let arg_values = emit_call_arguments(fn_ctx, arguments)?;
    for (i, arg_val) in arg_values.iter().enumerate() {
        let slot = fn_ctx.builder.build_struct_gep(
            child_struct_ty,
            child_ptr,
            1 + i as u32,
            "child_arg",
        )?;
        fn_ctx.builder.build_store(slot, arg_val.value)?;
    }

    let child_poll_name = if info.child_name == "sleep" {
        "ntsc_async_sleep_poll".to_string()
    } else {
        format!("ntsc_future_{}_poll", info.child_name)
    };
    let child_poll_fn = fn_ctx
        .module
        .get_function(&child_poll_name)
        .ok_or_else(|| {
            crate::CodegenError::LLVMError(format!(
                "internal: child poll {child_poll_name} not declared"
            ))
        })?;
    let poll_ptr = child_poll_fn.as_global_value().as_pointer_value();
    let poll_i8 = fn_ctx.builder.build_pointer_cast(
        poll_ptr,
        fn_ctx.context.ptr_type(AddressSpace::default()),
        "child_poll_fn",
    )?;
    let child_handle =
        fn_ctx
            .builder
            .build_ptr_to_int(child_ptr, fn_ctx.context.i64_type(), "child_handle")?;
    let push_fn = fn_ctx
        .module
        .get_function("ntsc_async_push")
        .ok_or_else(|| crate::CodegenError::LLVMError("ntsc_async_push not declared".into()))?;
    fn_ctx.builder.build_call(
        push_fn,
        &[poll_i8.into(), child_handle.into()],
        "async_push",
    )?;

    let state_ptr = fn_ctx.future_field(0)?;
    let next_state = fn_ctx
        .context
        .i32_type()
        .const_int((layout.await_state_index[await_idx] as u64) + 1, false);
    fn_ctx.builder.build_store(state_ptr, next_state)?;
    fn_ctx
        .builder
        .build_return(Some(&fn_ctx.context.i8_type().const_int(0, false)))?;
    Ok(())
}

/// On resume, load the awaited child's result from its future struct and
/// coerce it to the await's declared type; the sleeping path drops the
/// completed runtime sleep handle.
pub(crate) fn emit_await_resume<'ctx>(
    fn_ctx: &mut FunctionContext<'ctx, '_>,
    layout: &AsyncLayout,
    body: &[Stmt],
    await_idx: usize,
) -> Result<(), crate::CodegenError> {
    let info = &layout.await_infos[await_idx];
    let stmt = &body[info.stmt_idx];

    if info.child_name == "sleep" {
        let child_slot = fn_ctx.future_field(layout.sub_field_base + await_idx as u32)?;
        let handle = fn_ctx
            .builder
            .build_load(fn_ctx.context.i64_type(), child_slot, "sleep_handle")?
            .into_int_value();
        let drop_fn = fn_ctx
            .module
            .get_function("ntsc_async_sleep_drop")
            .ok_or_else(|| {
                crate::CodegenError::LLVMError("ntsc_async_sleep_drop not declared".into())
            })?;
        fn_ctx
            .builder
            .build_call(drop_fn, &[handle.into()], "sleep_drop")?;

        fn_ctx
            .builder
            .build_store(child_slot, fn_ctx.context.i64_type().const_zero())?;
        if !matches!(stmt, Stmt::Expression { .. }) {
            return Err(crate::CodegenError::LLVMError(
                "internal: void await consumed by a value statement".into(),
            ));
        }
        return Ok(());
    }

    let child_struct_name = format!("ntsc_future_{}", info.child_name);
    let child_struct_ty = fn_ctx
        .module
        .get_struct_type(&child_struct_name)
        .ok_or_else(|| {
            crate::CodegenError::LLVMError(format!(
                "internal: child future {child_struct_name} not declared"
            ))
        })?;
    let child_slot = fn_ctx.future_field(layout.sub_field_base + await_idx as u32)?;
    let child_ptr = fn_ctx.builder.build_pointer_cast(
        child_slot,
        fn_ctx.context.ptr_type(AddressSpace::default()),
        "child_future",
    )?;

    let result_slot = fn_ctx.builder.build_struct_gep(
        child_struct_ty,
        child_ptr,
        info.child_result_index,
        "child_result",
    )?;
    let child_result_ty = child_struct_ty
        .get_field_type_at_index(info.child_result_index)
        .ok_or_else(|| {
            crate::CodegenError::LLVMError("internal: missing child result field".into())
        })?;
    let child_result =
        fn_ctx
            .builder
            .build_load(child_result_ty, result_slot, "child_result_val")?;

    if ty_is_owned_handle(&info.child_ret_ty) {
        fn_ctx.builder.build_store(
            result_slot,
            default_llvm_value(&info.child_ret_ty, fn_ctx.context),
        )?;
    }

    match stmt {
        Stmt::Expression { .. } => {}
        Stmt::Var {
            name,
            type_annotation,
            ..
        } => {
            let slot_ty = type_annotation_to_ty(type_annotation);
            let field_index = layout.fields.get(name.lexeme()).copied().ok_or_else(|| {
                crate::CodegenError::LLVMError(format!(
                    "internal: awaited variable `{}` has no future field",
                    name.lexeme()
                ))
            })?;
            let slot = fn_ctx.future_field(field_index)?;
            fn_ctx.define_var(name.lexeme(), slot, slot_ty.clone());
            let coerced = coerce_value(
                fn_ctx,
                TypedValue::new(child_result, info.child_ret_ty.clone()),
                &slot_ty,
            )?;
            fn_ctx.builder.build_store(slot, coerced.value)?;
        }
        Stmt::Return { .. } => {
            let coerced = coerce_value(
                fn_ctx,
                TypedValue::new(child_result, info.child_ret_ty.clone()),
                &layout.ret_ty,
            )?;
            emit_async_return(fn_ctx, layout, Some(coerced.value))?;
        }
        _ => {
            return Err(crate::CodegenError::LLVMError(
                "internal: unexpected await statement shape".into(),
            ));
        }
    }
    Ok(())
}

/// Suspend on a channel send/receive `chan_idx`: evaluate the channel (and
/// value), park the goroutine via `ntask_chan_send`/`ntask_chan_recv`, store
/// the resume state, and return 0. On the next poll the scheduler has
/// completed the op and `emit_chan_resume` reads any received value.
pub(crate) fn emit_chan_suspend<'ctx>(
    fn_ctx: &mut FunctionContext<'ctx, '_>,
    layout: &AsyncLayout,
    body: &[Stmt],
    chan_idx: usize,
) -> Result<(), crate::CodegenError> {
    let info = &layout.chan_infos[chan_idx];
    let stmt = &body[info.stmt_idx];
    let channel_expr = chan_stmt_channel(stmt)?;
    let channel = emit_expression(fn_ctx, channel_expr)?;
    let channel_arg = channel.value.into_int_value();

    let result = match info.op {
        ChanOpKind::Send => {
            let value_expr = chan_stmt_value(stmt)?;
            let value = emit_expression(fn_ctx, value_expr)?;
            // A send moves the value into the channel: null the source
            // variable's slot when the value was a named owned handle so the
            // goroutine's drop path does not double-free it.
            if let Expr::Variable { name } = value_expr
                && ty_is_owned_handle(&value.ntsc_type)
                && let Some((ptr, _)) = fn_ctx.variables.get(name.lexeme())
            {
                fn_ctx
                    .builder
                    .build_store(*ptr, default_llvm_value(&value.ntsc_type, fn_ctx.context))?;
            }
            let value_arg = value.value.into_int_value();
            let send_fn = fn_ctx
                .module
                .get_function("ntask_chan_send")
                .ok_or_else(|| {
                    crate::CodegenError::LLVMError("ntask_chan_send not declared".into())
                })?;
            fn_ctx.builder.build_call(
                send_fn,
                &[channel_arg.into(), value_arg.into()],
                "chan_send",
            )?
        }
        ChanOpKind::Recv => {
            let recv_fn = fn_ctx
                .module
                .get_function("ntask_chan_recv")
                .ok_or_else(|| {
                    crate::CodegenError::LLVMError("ntask_chan_recv not declared".into())
                })?;
            fn_ctx
                .builder
                .build_call(recv_fn, &[channel_arg.into()], "chan_recv")?
        }
    };
    let returned = call_result_to_value(fn_ctx, &result).into_int_value();
    // `ntask_chan_send`/`ntask_chan_recv` return 1 on an invalid channel
    // handle; assert so a corrupt handle surfaces as a panic.
    fn_ctx.builder.build_call(
        fn_ctx
            .module
            .get_function("ntsc_assert")
            .ok_or_else(|| crate::CodegenError::LLVMError("ntsc_assert not declared".into()))?,
        &[
            returned.into(),
            fn_ctx.context.i64_type().const_zero().into(),
        ],
        "chan_handle_ok",
    )?;

    let state_ptr = fn_ctx.future_field(0)?;
    let next_state = fn_ctx
        .context
        .i32_type()
        .const_int((layout.chan_state_index[chan_idx] as u64) + 1, false);
    fn_ctx.builder.build_store(state_ptr, next_state)?;
    fn_ctx
        .builder
        .build_return(Some(&fn_ctx.context.i8_type().const_int(0, false)))?;
    Ok(())
}

/// On resume after a channel receive, read the completed value through
/// `ntask_chan_recv_result`, coerce it to the statement's target type, and
/// store it (or return it). A send has nothing to resume.
pub(crate) fn emit_chan_resume<'ctx>(
    fn_ctx: &mut FunctionContext<'ctx, '_>,
    layout: &AsyncLayout,
    body: &[Stmt],
    chan_idx: usize,
) -> Result<(), crate::CodegenError> {
    let info = &layout.chan_infos[chan_idx];
    if info.op == ChanOpKind::Send {
        return Ok(());
    }
    let stmt = &body[info.stmt_idx];

    let result_fn = fn_ctx
        .module
        .get_function("ntask_chan_recv_result")
        .ok_or_else(|| {
            crate::CodegenError::LLVMError("ntask_chan_recv_result not declared".into())
        })?;
    let result = fn_ctx
        .builder
        .build_call(result_fn, &[], "chan_recv_result")?;
    let received = call_result_to_value(fn_ctx, &result);

    let receiver = chan_stmt_receiver(stmt)?;
    let receiver_name = receiver.lexeme();
    let field_index = layout.fields.get(receiver_name).copied().ok_or_else(|| {
        crate::CodegenError::LLVMError(format!(
            "internal: received variable `{receiver_name}` has no future field"
        ))
    })?;
    let slot = fn_ctx.future_field(field_index)?;
    fn_ctx.define_var(receiver_name, slot, info.recv_ty.clone());
    let coerced = coerce_value(
        fn_ctx,
        TypedValue::new(received, info.recv_ty.clone()),
        &info.recv_ty,
    )?;
    fn_ctx.builder.build_store(slot, coerced.value)?;
    Ok(())
}

/// Complete the future: store the result, set the done state, return 1.
pub(crate) fn emit_async_return<'ctx>(
    fn_ctx: &mut FunctionContext<'ctx, '_>,
    layout: &AsyncLayout,
    value: Option<BasicValueEnum<'ctx>>,
) -> Result<(), crate::CodegenError> {
    if let Some(value) = value {
        let result_ptr = fn_ctx.future_field(layout.result_index)?;
        fn_ctx.builder.build_store(result_ptr, value)?;
    }
    let state_ptr = fn_ctx.future_field(0)?;
    let done_state = fn_ctx
        .context
        .i32_type()
        .const_int((layout.suspension_count as u64) + 1, false);
    fn_ctx.builder.build_store(state_ptr, done_state)?;
    fn_ctx
        .builder
        .build_return(Some(&fn_ctx.context.i8_type().const_int(1, false)))?;
    Ok(())
}

/// Synchronous `__ntsc_user_main` for an async `main`: stack-allocate and
/// zero the root future, drive it through `ntsc_async_run`, and return the
/// stored result (the C `main` wrapper truncates it to the exit code).
pub(crate) fn emit_async_main_wrapper<'ctx>(
    context: &'ctx Context,
    module: &Module<'ctx>,
    struct_name: &str,
    layout: &AsyncLayout,
) -> Result<(), crate::CodegenError> {
    let future_ty = module.get_struct_type(struct_name).ok_or_else(|| {
        crate::CodegenError::LLVMError(format!("internal: missing future struct {struct_name}"))
    })?;
    let future_size = future_ty.size_of().ok_or_else(|| {
        crate::CodegenError::LLVMError(format!("internal: {struct_name} has no size"))
    })?;

    let ret_llvm = ty_to_llvm(&layout.ret_ty, context);
    let wrapper_ty = if layout.ret_ty == Ty::Void {
        context.void_type().fn_type(&[], false)
    } else {
        ret_llvm.fn_type(&[], false)
    };
    let wrapper = module.add_function(
        "__ntsc_user_main",
        wrapper_ty,
        Some(inkwell::module::Linkage::External),
    );
    let entry = context.append_basic_block(wrapper, "entry");
    let builder = context.create_builder();
    builder.position_at_end(entry);
    let entry_builder = context.create_builder();
    entry_builder.position_at_end(entry);

    let fn_ctx = FunctionContext::new(
        wrapper,
        &builder,
        &entry_builder,
        entry,
        module,
        Ty::Void,
        context,
    );

    let future = fn_ctx.builder.build_alloca(future_ty, "future")?;

    let zero = fn_ctx.context.i8_type().const_zero();
    fn_ctx.builder.build_memset(future, 1, zero, future_size)?;

    let run_fn = fn_ctx
        .module
        .get_function("ntsc_async_run")
        .ok_or_else(|| crate::CodegenError::LLVMError("ntsc_async_run not declared".into()))?;
    let poll_fn = fn_ctx
        .module
        .get_function(&format!("ntsc_future_{}_poll", layout.name))
        .ok_or_else(|| crate::CodegenError::LLVMError("missing main poll function".into()))?;
    let poll_i8 = fn_ctx.builder.build_pointer_cast(
        poll_fn.as_global_value().as_pointer_value(),
        fn_ctx.context.ptr_type(AddressSpace::default()),
        "poll_fn",
    )?;
    let future_handle =
        fn_ctx
            .builder
            .build_ptr_to_int(future, fn_ctx.context.i64_type(), "future_handle")?;
    fn_ctx
        .builder
        .build_call(run_fn, &[poll_i8.into(), future_handle.into()], "async_run")?;

    let result = if layout.ret_ty == Ty::Void {
        None
    } else {
        let result_ptr = fn_ctx.builder.build_struct_gep(
            future_ty,
            future,
            layout.result_index,
            "result_ptr",
        )?;
        let result = fn_ctx
            .builder
            .build_load(ret_llvm, result_ptr, "main_result")?;
        Some(result)
    };

    if let Some(result) = result {
        if ty_is_owned_handle(&layout.ret_ty) {
            fn_ctx.builder.build_store(
                fn_ctx.builder.build_struct_gep(
                    future_ty,
                    future,
                    layout.result_index,
                    "result_drop_slot",
                )?,
                default_llvm_value(&layout.ret_ty, context),
            )?;
        }
        let drop_fn = fn_ctx
            .module
            .get_function(&format!("{struct_name}_drop"))
            .ok_or_else(|| {
                crate::CodegenError::LLVMError("missing main future drop function".into())
            })?;
        fn_ctx
            .builder
            .build_call(drop_fn, &[future.into()], "async_future_drop")?;
        fn_ctx.builder.build_return(Some(&result))?;
    } else {
        let drop_fn = fn_ctx
            .module
            .get_function(&format!("{struct_name}_drop"))
            .ok_or_else(|| {
                crate::CodegenError::LLVMError("missing main future drop function".into())
            })?;
        fn_ctx
            .builder
            .build_call(drop_fn, &[future.into()], "async_future_drop")?;
        fn_ctx.builder.build_return(None)?;
    }
    Ok(())
}
