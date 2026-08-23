//! Statement emission inside function bodies.

use super::*;

pub(crate) fn emit_statement_in_function<'ctx>(
    fn_ctx: &mut FunctionContext<'ctx, '_>,
    stmt: &Stmt,
) -> Result<(), crate::CodegenError> {
    // Skip statements after a terminator (dead code after return/throw).
    if let Some(block) = fn_ctx.builder.get_insert_block()
        && block.get_terminator().is_some()
    {
        return Ok(());
    }
    match stmt {
        Stmt::Say { expression, .. } => {
            emit_say_call(fn_ctx, expression)?;
        }
        Stmt::Expression { expression } => {
            let val = emit_expression(fn_ctx, expression)?;
            // A fresh expression result owns nobody's slot: reclaim it now.
            if expr_is_fresh(fn_ctx, expression, &val) {
                emit_drop_value(fn_ctx, &val)?;
            }
        }
        Stmt::Return { value } => {
            if fn_ctx.return_type == Ty::Void {
                if let Some(expr) = value {
                    // A value returned from a void function is evaluated for
                    // side effects and discarded.
                    let val = emit_expression(fn_ctx, expr)?;
                    if expr_is_owned(fn_ctx, expr, &val) {
                        emit_drop_value(fn_ctx, &val)?;
                    }
                }
                emit_drop_all_owned(fn_ctx)?;
                fn_ctx.builder.build_return(None)?;
            } else if let Some(expr) = value {
                let val = emit_expression(fn_ctx, expr)?;
                let ret_ty = fn_ctx.return_type.clone();
                let val_ty = val.ntsc_type.clone();
                let converted = convert_to_expected(fn_ctx, val, &ret_ty)?;

                // A class instance returned as a trait object is wrapped in
                // an owning fat pointer; only a construction may be adopted,
                // or the same instance would end up owned twice.
                let returns_dyn = match &ret_ty {
                    Ty::Dyn(_) => true,
                    Ty::Own(inner) => matches!(inner.as_ref(), Ty::Dyn(_)),
                    _ => false,
                };
                let already_dyn = match &converted.ntsc_type {
                    Ty::Dyn(_) => true,
                    Ty::Own(inner) => matches!(inner.as_ref(), Ty::Dyn(_)),
                    _ => false,
                };
                let converted = if returns_dyn && !already_dyn {
                    if !super::dyn_obj::expr_is_fresh_construction(fn_ctx, expr) {
                        return Err(crate::CodegenError::LLVMError(
                            "only a newly constructed instance can become a trait object".into(),
                        ));
                    }
                    coerce_value(fn_ctx, converted, &ret_ty)?
                } else {
                    converted
                };

                // A string literal handed to the caller must be heap-copied
                // so the caller owns a value it can drop.
                let converted = if expr_is_string_literal(expr) && matches!(ret_ty, Ty::String) {
                    TypedValue::new(clone_string_value(fn_ctx, &converted)?, ret_ty.clone())
                } else {
                    converted
                };

                // A shared return must hand the caller a box reference:
                // * a shared variable slot still owns its reference (it is
                //   released at exit), so retain once for the caller,
                // * an owned value or literal is boxed (adopted); a
                //   bare-variable source is moved and nulled below.
                let converted = if matches!(ret_ty, Ty::Shared(_)) {
                    match &val_ty {
                        Ty::Shared(_) => {
                            let retain_fn = fn_ctx
                                .module
                                .get_function("ntsc_shared_retain")
                                .ok_or_else(|| {
                                    crate::CodegenError::LLVMError(
                                        "ntsc_shared_retain not declared".into(),
                                    )
                                })?;
                            fn_ctx.builder.build_call(
                                retain_fn,
                                &[BasicMetadataValueEnum::PointerValue(
                                    converted.value.into_pointer_value(),
                                )],
                                "shared_ret_return",
                            )?;
                            converted
                        }
                        _ => box_or_retain_shared(fn_ctx, &ret_ty, expr, &converted)?,
                    }
                } else {
                    converted
                };

                // A borrowed owned return (e.g. `return this.field`) must
                // hand the caller an independent copy: the source keeps its
                // own reference, so the caller's drop and the scope's class
                // drop never reclaim the same allocation. Bare variables
                // move instead (nulled below); fresh values transfer
                // directly.
                let converted = if matches!(ret_ty, Ty::Array(_) | Ty::String)
                    && !expr_is_string_literal(expr)
                    && !expr_is_fresh(fn_ctx, expr, &converted)
                    && !matches!(expr, Expr::Variable { .. })
                {
                    copy_owned_value(fn_ctx, &converted)?
                } else {
                    converted
                };

                // The returned value is moved out of its slot: null it so
                // the exit-time drop does not free it twice. Shared slots
                // are never nulled — their reference was handed to the
                // caller by the retain above, and the slot still releases
                // its own at exit.
                if let Expr::Variable { name } = expr {
                    fn_ctx.null_var_slot(name.lexeme());
                }
                emit_drop_all_owned(fn_ctx)?;
                fn_ctx.builder.build_return(Some(&converted.value))?;
            } else {
                emit_drop_all_owned(fn_ctx)?;
                fn_ctx.builder.build_return(None)?;
            }
        }
        Stmt::Var {
            name,
            type_annotation,
            initializer,
            view,
            ..
        } => {
            let view_mut = matches!(view, Some(ntsc_ast::types::ViewMutability::Mutable));

            if view.is_none()
                && type_annotation.is_none()
                && let Some(init_expr) = initializer
                && fn_ctx.stack_allocated.contains(name.lexeme())
                && let Expr::Call {
                    callee, arguments, ..
                } = init_expr
                && arguments.is_empty()
                && let Expr::Variable { name: ctor } = callee.as_ref()
                && let Some(struct_ty) = fn_ctx.module.get_struct_type(ctor.lexeme())
            {
                // Escape-analysis path: `var x = ClassName()` (no args, no
                // init) whose object never escapes is stack-allocated into
                // a slot.
                let slot =
                    fn_ctx.alloca_llvm(&format!("slot_{}", name.lexeme()), struct_ty.into())?;
                let obj =
                    emit_class_constructor(fn_ctx, struct_ty, ctor.lexeme(), &[], &[], Some(slot))?;

                // A user `init` method can throw even with no arguments.
                fn_ctx.emit_pending_exception_check()?;
                let ptr = fn_ctx.alloca(name.lexeme(), &Ty::Class(ctor.lexeme().to_string()))?;
                fn_ctx.builder.build_store(ptr, obj.value)?;
                fn_ctx.define_var(name.lexeme(), ptr, obj.ntsc_type.clone());

                fn_ctx.mark_owned_if_heap(name.lexeme(), &obj.ntsc_type);
                return Ok(());
            }

            let ann_ty = type_annotation_to_ty(type_annotation);
            let (ty, ptr, owned) = if let Some(init_expr) = initializer {
                let init_val = emit_expression(fn_ctx, init_expr)?;
                let inferred = if view.is_some() {
                    // A view declaration borrows the annotated (or
                    // initializer) inner type; a view of a shared value
                    // borrows the wrapped pointee, never the box. A view of
                    // an existing view aliases it.
                    let inner = if matches!(ann_ty, Ty::Any) {
                        match &init_val.ntsc_type {
                            Ty::View(inner, _) => (**inner).clone(),

                            Ty::Shared(inner) => (**inner).clone(),
                            other => other.clone(),
                        }
                    } else {
                        ann_ty
                    };
                    Ty::View(Box::new(inner), view_mut)
                } else if type_annotation.is_some() {
                    ann_ty
                } else {
                    init_val.ntsc_type.clone()
                };
                let ptr = fn_ctx.alloca(name.lexeme(), &inferred)?;

                if matches!(&inferred, Ty::View(..)) {
                    // Views are non-owning borrows: store the raw value
                    // without transferring ownership or nulling the source
                    // slot. A shared source is dereferenced so the view
                    // points at the wrapped value, not the box.
                    let init_val = deref_shared(fn_ctx, init_val)?;
                    fn_ctx.builder.build_store(ptr, init_val.value)?;
                    (inferred, ptr, false)
                } else {
                    // One entry-block slot backs this declaration on every
                    // pass, so a declaration inside a loop body must release
                    // what the slot still holds from the previous iteration
                    // or every value but the last leaks. `alloca`
                    // null-initializes these slots, so the first pass drops
                    // a null handle, which is a no-op. A redeclaration whose
                    // initializer reads the slot it reuses hands the same
                    // handle back, which the identity guard skips. Async
                    // locals live in the future struct, whose lifetime the
                    // executor owns, and are skipped like they are at
                    // function exit.
                    if matches!(
                        inferred,
                        Ty::Array(_)
                            | Ty::String
                            | Ty::Object
                            | Ty::Shared(_)
                            | Ty::Option(_)
                            | Ty::Dyn(_)
                    ) && fn_ctx.future_base.is_none()
                    {
                        emit_drop_replaced_value(fn_ctx, ptr, &inferred, &init_val)?;
                    }

                    // A class slot is only reclaimed here when the analysis
                    // proved this instance is neither aliased nor escaping
                    // (`class_drops`); otherwise another name may still read
                    // the instance and its fields are deliberately leaked
                    // instead of freed twice. The thunk is a no-op on the
                    // null pointer the slot starts out holding.
                    if matches!(inferred, Ty::Class(_))
                        && (fn_ctx.class_drops.contains(name.lexeme())
                            || expr_is_fresh(fn_ctx, init_expr, &init_val))
                    {
                        emit_drop_slot_value(fn_ctx, ptr, &inferred)?;
                    }
                    let coerced = coerce_value(fn_ctx, init_val, &inferred)?;

                    correct_empty_array_flag(fn_ctx, init_expr, &coerced, &inferred)?;
                    // Owned-store semantics: transfer owned values,
                    // heap-copy string literals, move bare-variable sources.
                    let owned = store_into_owned_slot(fn_ctx, ptr, &inferred, init_expr, &coerced)?;
                    if owned
                        && matches!(
                            inferred,
                            Ty::Array(_) | Ty::String | Ty::Object | Ty::Shared(_)
                        )
                        && let Some(mark_fn) = fn_ctx.module.get_function("ntsc_leak_mark")
                    {
                        let line = fn_ctx
                            .context
                            .i64_type()
                            .const_int(u64::from(name.span.line), false);
                        let column = fn_ctx
                            .context
                            .i64_type()
                            .const_int(u64::from(name.span.column), false);
                        fn_ctx.builder.build_call(
                            mark_fn,
                            &[
                                coerced.value.into_int_value().into(),
                                line.into(),
                                column.into(),
                            ],
                            "leak_mark",
                        )?;
                    }
                    (inferred, ptr, owned)
                }
            } else {
                let ptr = fn_ctx.alloca(name.lexeme(), &ann_ty)?;
                (ann_ty, ptr, false)
            };
            if (owned || fn_ctx.class_drops.contains(name.lexeme())) && fn_ctx.future_base.is_none()
            {
                fn_ctx.mark_owned_if_heap(name.lexeme(), &ty);
            }
            fn_ctx.define_var(name.lexeme(), ptr, ty);
        }
        Stmt::Block { statements, .. } => {
            let outer = fn_ctx.begin_block_scope();
            for s in statements {
                emit_statement_in_function(fn_ctx, s)?;
            }
            fn_ctx.end_block_scope(outer);
        }
        Stmt::If {
            condition,
            then_branch,
            elif_branches,
            else_branch,
        } => {
            emit_if(fn_ctx, condition, then_branch, elif_branches, else_branch)?;
        }
        Stmt::While { condition, body } => {
            emit_while(fn_ctx, condition, body)?;
        }
        Stmt::DoWhile { body, condition } => {
            emit_do_while(fn_ctx, condition, body)?;
        }
        Stmt::For {
            init,
            condition,
            update,
            body,
        } => {
            emit_for(fn_ctx, init, condition, update, body)?;
        }
        Stmt::ForIn {
            variable,
            iterable,
            body,
        } => {
            emit_for_in(fn_ctx, variable, iterable, body)?;
        }
        Stmt::Break { .. } => {
            fn_ctx.emit_break()?;
        }
        Stmt::Continue { .. } => {
            fn_ctx.emit_continue()?;
        }
        Stmt::Throw { value } => {
            let val = emit_expression(fn_ctx, value)?;
            let msg = convert_to_string(fn_ctx, &val)?;
            // `ntsc_throw` consumes the message handle and sets the pending
            // exception; a string literal is cloned first because the
            // runtime may free the message on propagation.
            let msg_handle = if expr_is_string_literal(value) {
                clone_string_value(fn_ctx, &msg)?
            } else {
                msg.value
            };
            let throw_fn = fn_ctx
                .module
                .get_function("ntsc_throw")
                .ok_or_else(|| crate::CodegenError::LLVMError("ntsc_throw not declared".into()))?;

            fn_ctx.builder.build_call(
                throw_fn,
                &[BasicMetadataValueEnum::IntValue(
                    msg_handle.into_int_value(),
                )],
                "throw_call",
            )?;

            // Throwing a string variable — `throw e` rethrowing a catch
            // binding, most often — moves the message out of its slot.
            // Without nulling the slot, the unwind path's exit drop frees
            // the handle that is now the pending exception, and the next
            // handler reads a reclaimed message. Any other thrown
            // expression produced a fresh string (a converted scalar, a
            // concatenation, a cloned literal), which no slot owns.
            if matches!(val.ntsc_type, Ty::String)
                && !expr_is_string_literal(value)
                && let Expr::Variable { name } = value
            {
                fn_ctx.null_var_slot(name.lexeme());
            }
            let handler = fn_ctx.current_exception_handler();
            fn_ctx.builder.build_unconditional_branch(handler)?;
        }
        Stmt::Match {
            expression,
            cases,
            default_case,
        } => {
            emit_match(fn_ctx, expression, cases, default_case)?;
        }
        Stmt::Try {
            try_block,
            catch_var,
            catch_block,
            finally_block,
            ..
        } => {
            emit_try_catch(fn_ctx, try_block, catch_var, catch_block, finally_block)?;
        }
        Stmt::Destructure {
            is_array,
            names,
            keys,
            initializer,
        } => {
            emit_destructure(fn_ctx, *is_array, names, keys, initializer)?;
        }
        Stmt::Unsafe { body } => {
            emit_statement_in_function(fn_ctx, body)?;
        }
        Stmt::Quiet { body, .. } => {
            emit_statement_in_function(fn_ctx, body)?;
        }
        Stmt::Retry {
            count,
            body,
            catch_var,
            catch_block,
        } => {
            emit_retry(fn_ctx, count, body, catch_var, catch_block)?;
        }
        Stmt::Enum { .. }
        | Stmt::TypeAlias { .. }
        | Stmt::Use { .. }
        | Stmt::Function { .. }
        | Stmt::AsyncFunction { .. }
        | Stmt::Class { .. }
        | Stmt::Test { .. }
        | Stmt::Trait { .. }
        | Stmt::Impl { .. } => {}
    }
    Ok(())
}
