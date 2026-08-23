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
    ret_ty: Ty,
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
        Stmt::ForIn { variable, body, .. } => {
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
            let (child_name, child_ret_ty) = await_callee_info(stmt, program)?;
            let child_param_count = await_callee_param_count(program, &child_name)?;
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
            await_infos.push(AwaitInfo {
                stmt_idx,
                child_name,
                child_ret_ty,
                child_result_index: 1 + child_param_count as u32,
            });
        }
    }

    Ok(AsyncLayout {
        name: name.lexeme().to_string(),
        field_tys,
        fields,
        result_index,
        sub_field_base,
        await_infos,
        ret_ty,
    })
}

pub(crate) fn await_callee_info(
    stmt: &Stmt,
    program: &Program,
) -> Result<(String, Ty), crate::CodegenError> {
    let (callee_expr, _arguments) = await_stmt_parts(stmt)?;
    let callee_name = match callee_expr {
        Expr::Variable { name } => name.lexeme(),
        Expr::Member { object, property } if matches!(object.as_ref(), Expr::Variable { name } if name.lexeme() == "async") => {
            property.lexeme()
        }
        _ => {
            return Err(crate::CodegenError::LLVMError(
                "await requires a call to a module-level async function".into(),
            ));
        }
    };
    if callee_name == "sleep" {
        return Ok(("sleep".to_string(), Ty::Void));
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
    Ok((callee_name.to_string(), ret_ty))
}

pub(crate) fn await_callee_param_count(
    program: &Program,
    child_name: &str,
) -> Result<usize, crate::CodegenError> {
    if child_name == "sleep" {
        return Ok(1);
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

    // Emit awaited callees first so their future struct types exist when
    // this future's fields reference them (reverse topological order). The
    // built-in `async.sleep` has no emitted callee: its future struct and
    // poll function are declared as part of the runtime.
    for info in &layout.await_infos {
        if info.child_name == "sleep" {
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

    let seg_count = layout.await_infos.len() + 1;
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
    let await_infos = &layout.await_infos;
    let (start, end, resume_await) = if segment_index == 0 {
        let end = await_infos
            .first()
            .map(|info| info.stmt_idx)
            .unwrap_or(body.len());
        (0, end, None)
    } else {
        let prev_await = await_infos[segment_index - 1].stmt_idx;
        let end = await_infos
            .get(segment_index)
            .map(|info| info.stmt_idx)
            .unwrap_or(body.len());
        (prev_await + 1, end, Some(segment_index - 1))
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

    if let Some(await_idx) = resume_await {
        emit_await_resume(fn_ctx, layout, body, await_idx)?;
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

    if segment_index < await_infos.len() {
        emit_await_suspend(fn_ctx, layout, body, segment_index)?;
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
            .const_int(1 + await_idx as u64, false);
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
        .const_int((await_idx as u64) + 1, false);
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
        .const_int((layout.await_infos.len() as u64) + 1, false);
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
