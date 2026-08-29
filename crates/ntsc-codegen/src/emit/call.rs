//! Call emission: argument preparation, spread, stdlib calls.

use super::*;

/// Get a basic value result from a call site. A `void` return produces no
/// basic value; callers still get a placeholder so call expressions can be
/// used in statement position.
pub(crate) fn call_result_to_value<'ctx>(
    fn_ctx: &FunctionContext<'ctx, '_>,
    result: &inkwell::values::CallSiteValue<'ctx>,
) -> BasicValueEnum<'ctx> {
    result
        .try_as_basic_value()
        .basic()
        .unwrap_or_else(|| default_llvm_value(&Ty::Any, fn_ctx.context))
}

/// Map a stdlib ABI function name to its return type. Most modules' C
/// functions return a fixed type (all `ntsc_math_*` return `f64`, ...);
/// everything else defaults to `Any` and stdlib call sites then re-derive
/// the type from their own static table.
pub(crate) fn stdlib_return_ty(name: &str) -> Ty {
    if name.starts_with("ntsc_math_") {
        return Ty::Float;
    }
    match name {
        "ntsc_fmt_to_int" | "ntsc_fmt_to_hex" | "ntsc_fmt_to_oct" => Ty::Int,
        "ntsc_fmt_to_float" => Ty::Float,
        "ntsc_fmt_i64_to_str"
        | "ntsc_fmt_f64_to_str"
        | "ntsc_fmt_type_name"
        | "ntsc_fmt_pad_left"
        | "ntsc_fmt_pad_right" => Ty::String,
        "ntsc_fmt_is_int" | "ntsc_fmt_is_float" => Ty::Bool,

        "ntsc_time_now" => Ty::Float,
        "ntsc_time_format" => Ty::String,
        "ntsc_time_sleep" => Ty::Void,

        "ntsc_sys_read" | "ntsc_sys_listdir" | "ntsc_sys_cwd" | "ntsc_sys_env"
        | "ntsc_sys_args" | "ntsc_sys_walk" | "ntsc_sys_readlink" => Ty::String,
        "ntsc_sys_write"
        | "ntsc_sys_append"
        | "ntsc_sys_exists"
        | "ntsc_sys_mkdir"
        | "ntsc_sys_cp"
        | "ntsc_sys_rm"
        | "ntsc_sys_symlink"
        | "ntsc_sys_is_symlink" => Ty::Bool,
        "ntsc_sys_exit" | "ntsc_sys_sleep" => Ty::Void,
        "ntsc_sys_exec" => Ty::Int,

        "ntsc_strings_contains"
        | "ntsc_strings_starts_with"
        | "ntsc_strings_ends_with"
        | "ntsc_strings_is_empty"
        | "ntsc_strings_is_alpha"
        | "ntsc_strings_is_digit"
        | "ntsc_strings_is_alnum" => Ty::Bool,
        "ntsc_strings_length"
        | "ntsc_strings_count"
        | "ntsc_strings_index_of"
        | "ntsc_strings_last_index_of"
        | "ntsc_strings_char_code" => Ty::Int,
        "ntsc_strings_split"
        | "ntsc_strings_join"
        | "ntsc_strings_trim"
        | "ntsc_strings_trim_left"
        | "ntsc_strings_trim_right"
        | "ntsc_strings_upper"
        | "ntsc_strings_lower"
        | "ntsc_strings_replace"
        | "ntsc_strings_replace_first"
        | "ntsc_strings_substring"
        | "ntsc_strings_repeat"
        | "ntsc_strings_reverse"
        | "ntsc_strings_char_at"
        | "ntsc_strings_from_char_code" => Ty::String,

        "ntsc_csv_parse"
        | "ntsc_csv_stringify"
        | "ntsc_toml_parse"
        | "ntsc_toml_stringify"
        | "ntsc_yaml_parse"
        | "ntsc_yaml_stringify"
        | "ntsc_json_parse"
        | "ntsc_json_stringify"
        | "ntsc_json_get"
        | "ntsc_json_keys"
        | "ntsc_json_stringify_pretty" => Ty::String,
        "ntsc_json_is_valid" | "ntsc_json_has" => Ty::Bool,

        "ntsc_testing_bench" => Ty::Float,

        "ntsc_http_get"
        | "ntsc_http_post"
        | "ntsc_http_put"
        | "ntsc_http_delete"
        | "ntsc_http_head"
        | "ntsc_http_patch"
        | "ntsc_http_request"
        | "ntsc_http_get_range"
        | "ntsc_http_get_file"
        | "ntsc_http_download_with_progress"
        | "ntsc_http_concurrent_download" => Ty::String,
        "ntsc_http_status_code" => Ty::Int,

        "ntsc_crypto_base64_encode"
        | "ntsc_crypto_base64_decode"
        | "ntsc_crypto_sha256"
        | "ntsc_crypto_sha512"
        | "ntsc_crypto_sha384"
        | "ntsc_crypto_sha224"
        | "ntsc_crypto_md5"
        | "ntsc_crypto_hmac_sha256"
        | "ntsc_crypto_hmac_sha512"
        | "ntsc_crypto_hex_encode"
        | "ntsc_crypto_hex_decode"
        | "ntsc_crypto_random_bytes"
        | "ntsc_crypto_random_string"
        | "ntsc_crypto_xor_cipher"
        | "ntsc_crypto_file_sha256"
        | "ntsc_crypto_file_sha512" => Ty::String,
        "ntsc_crypto_verify_sha256" | "ntsc_crypto_verify_sha512" => Ty::Bool,

        "ntsc_collections_set_new"
        | "ntsc_collections_set_to_array"
        | "ntsc_collections_set_union"
        | "ntsc_collections_set_intersection"
        | "ntsc_collections_set_difference"
        | "ntsc_collections_stack_new"
        | "ntsc_collections_queue_new" => Ty::String,
        "ntsc_collections_set_add"
        | "ntsc_collections_set_has"
        | "ntsc_collections_set_remove"
        | "ntsc_collections_stack_push"
        | "ntsc_collections_stack_pop"
        | "ntsc_collections_stack_peek"
        | "ntsc_collections_stack_is_empty"
        | "ntsc_collections_queue_enqueue"
        | "ntsc_collections_queue_dequeue"
        | "ntsc_collections_queue_peek"
        | "ntsc_collections_queue_is_empty"
        | "ntsc_collections_channel_send" => Ty::Bool,
        "ntsc_collections_set_size"
        | "ntsc_collections_stack_size"
        | "ntsc_collections_queue_size"
        | "ntsc_collections_channel"
        | "ntsc_collections_channel_sender" => Ty::Int,
        "ntsc_collections_channel_recv" | "ntsc_collections_channel_try_recv" => Ty::String,
        "ntsc_collections_channel_close" => Ty::Void,

        "ntsc_regex_test" | "ntsc_regex_search" | "ntsc_regex_is_valid" => Ty::Bool,
        "ntsc_regex_find"
        | "ntsc_regex_find_all"
        | "ntsc_regex_replace"
        | "ntsc_regex_split"
        | "ntsc_regex_escape" => Ty::String,

        "ntsc_arrays_length" | "ntsc_arrays_index_of" | "ntsc_arrays_range" => Ty::Int,
        "ntsc_arrays_new"
        | "ntsc_arrays_join"
        | "ntsc_arrays_reverse"
        | "ntsc_arrays_sort"
        | "ntsc_arrays_remove"
        | "ntsc_arrays_remove_at"
        | "ntsc_arrays_slice"
        | "ntsc_arrays_clear"
        | "ntsc_arrays_clone"
        | "ntsc_arrays_fill"
        | "ntsc_arrays_flat"
        | "ntsc_arrays_shuffle" => Ty::String,
        "ntsc_arrays_push" => Ty::Void,
        "ntsc_arrays_pop"
        | "ntsc_arrays_at"
        | "ntsc_arrays_contains"
        | "ntsc_arrays_every"
        | "ntsc_arrays_some" => Ty::Bool,

        "ntsc_process_exec" | "ntsc_process_pid" | "ntsc_process_spawn_thread" => Ty::Int,
        "ntsc_process_exec_output" | "ntsc_process_spawn" => Ty::String,
        "ntsc_process_thread_join" => Ty::Bool,

        "ntsc_os_separator"
        | "ntsc_os_temp_dir"
        | "ntsc_os_temp_path"
        | "ntsc_os_temp_file"
        | "ntsc_os_getenv"
        | "ntsc_os_path_join"
        | "ntsc_os_path_dirname"
        | "ntsc_os_path_basename"
        | "ntsc_os_path_ext"
        | "ntsc_os_path_stem"
        | "ntsc_os_path_abs" => Ty::String,
        "ntsc_os_setenv"
        | "ntsc_os_unsetenv"
        | "ntsc_os_has_env"
        | "ntsc_os_is_abs"
        | "ntsc_os_file_unlock" => Ty::Bool,
        "ntsc_os_file_lock" => Ty::Int,

        "ntsc_io_stdin" | "ntsc_io_stdout" | "ntsc_io_stderr" | "ntsc_io_open"
        | "ntsc_io_write" | "ntsc_io_write_line" | "ntsc_io_tell" => Ty::Int,
        "ntsc_io_close" | "ntsc_io_flush" | "ntsc_io_eof" | "ntsc_io_seek" => Ty::Bool,
        "ntsc_io_input" | "ntsc_io_read" | "ntsc_io_read_line" | "ntsc_io_read_all" => Ty::String,

        "ntsc_net_tcp_connect"
        | "ntsc_net_tcp_listen"
        | "ntsc_net_local_port"
        | "ntsc_net_tcp_accept"
        | "ntsc_net_send"
        | "ntsc_net_send_line"
        | "ntsc_net_udp_bind"
        | "ntsc_net_udp_send" => Ty::Int,
        "ntsc_net_recv" | "ntsc_net_recv_line" | "ntsc_net_udp_recv" => Ty::String,
        "ntsc_net_close" => Ty::Bool,

        "ntsc_encoding_base64_encode"
        | "ntsc_encoding_base64_decode"
        | "ntsc_encoding_hex_encode"
        | "ntsc_encoding_hex_decode" => Ty::String,
        "ntsc_encoding_utf8_valid" => Ty::Bool,

        "ntsc_hash_sha256" => Ty::String,
        "ntsc_hash_crc32" => Ty::Int,

        "ntsc_slices_of" | "ntsc_slices_sub" => Ty::Slice(Box::new(Ty::Any)),
        "ntsc_slices_length" | "ntsc_slices_get" => Ty::Int,
        "ntsc_slices_to_array" => Ty::Array(Box::new(Ty::Any)),
        "ntsc_slices_set" | "ntsc_slices_fill" | "ntsc_slices_copy_from" | "ntsc_slices_equal" => {
            Ty::Bool
        }

        "ntsc_memory_alloc" | "ntsc_memory_offset" => Ty::Pointer,
        "ntsc_memory_load8" | "ntsc_memory_load64" => Ty::Int,
        "ntsc_memory_store8" | "ntsc_memory_store64" => Ty::Bool,

        "ntsc_random_seed" | "ntsc_random_bool" => Ty::Bool,
        "ntsc_random_int" => Ty::Int,
        "ntsc_random_float" => Ty::Float,

        // ── paths module ─────────────────────────────────────────────────
        "ntsc_paths_join"
        | "ntsc_paths_parent"
        | "ntsc_paths_file_name"
        | "ntsc_paths_extension"
        | "ntsc_paths_with_extension"
        | "ntsc_paths_stem"
        | "ntsc_paths_absolute"
        | "ntsc_paths_relative"
        | "ntsc_paths_components"
        | "ntsc_paths_normalize" => Ty::String,
        "ntsc_paths_is_absolute" => Ty::Bool,

        // ── glob module ──────────────────────────────────────────────────
        "ntsc_glob_matches" | "ntsc_glob_is_match" => Ty::Bool,
        "ntsc_glob_find" => Ty::String,

        // ── archive module ───────────────────────────────────────────────
        "ntsc_archive_extract_tar_gz"
        | "ntsc_archive_extract_tar"
        | "ntsc_archive_extract_zip"
        | "ntsc_archive_list_tar"
        | "ntsc_archive_list_zip" => Ty::String,

        _ => Ty::Any,
    }
}

/// Emit call arguments, expanding `...array` spread arguments into one
/// element per array entry.
pub(crate) fn emit_call_arguments<'ctx>(
    fn_ctx: &mut FunctionContext<'ctx, '_>,
    arguments: &[Expr],
) -> Result<Vec<TypedValue<'ctx>>, crate::CodegenError> {
    let mut values = Vec::new();
    for argument in arguments {
        if let Expr::Spread { value, .. } = argument {
            values.extend(emit_spread_elements(fn_ctx, value)?);
        } else {
            values.push(emit_expression(fn_ctx, argument)?);
        }
    }
    Ok(values)
}

/// Emit the elements of a spread argument. The argument list of a call has a
/// statically known arity, so spread only works when the operand is an array
/// literal whose length is known at compile time; each element is then
/// emitted as a regular argument.
pub(crate) fn emit_spread_elements<'ctx>(
    fn_ctx: &mut FunctionContext<'ctx, '_>,
    value: &Expr,
) -> Result<Vec<TypedValue<'ctx>>, crate::CodegenError> {
    let Expr::ArrayLiteral { elements, .. } = value else {
        return Err(crate::CodegenError::LLVMError(
            "spread requires an array literal `...[...]`".into(),
        ));
    };
    elements
        .iter()
        .map(|e| emit_expression(fn_ctx, e))
        .collect()
}

/// Unroll array-literal spreads in an argument list into their element
/// expressions, so the argument count seen by ownership transfer matches the
/// number of evaluated values. Non-literal spreads are left untouched and
/// handled at emit time by `emit_spread_elements`.
pub(crate) fn flatten_call_arguments(arguments: &[Expr]) -> Vec<Expr> {
    let mut flat = Vec::new();
    for argument in arguments {
        match argument {
            Expr::Spread { value, .. } => match &**value {
                Expr::ArrayLiteral { elements, .. } => {
                    flat.extend(flatten_call_arguments(elements));
                }
                _ => flat.push(argument.clone()),
            },
            _ => flat.push(argument.clone()),
        }
    }
    flat
}

pub(crate) fn flatten_array_elements(elements: &[Expr]) -> Result<Vec<&Expr>, crate::CodegenError> {
    let mut flat = Vec::new();
    for element in elements {
        match element {
            Expr::Spread { value, .. } => match &**value {
                Expr::ArrayLiteral {
                    elements: inner, ..
                } => {
                    flat.extend(flatten_array_elements(inner)?);
                }
                _ => {
                    return Err(crate::CodegenError::LLVMError(
                        "spread requires an array literal `...[...]`".into(),
                    ));
                }
            },
            _ => flat.push(element),
        }
    }
    Ok(flat)
}

pub(crate) fn emit_call<'ctx>(
    fn_ctx: &mut FunctionContext<'ctx, '_>,
    callee: &Expr,
    arguments: &[Expr],
) -> Result<TypedValue<'ctx>, crate::CodegenError> {
    let arguments = flatten_call_arguments(arguments);

    // `memory.raw_address(reference)` reinterprets a checked reference as a
    // raw pointer. Both are the same machine address, so this only changes
    // the static type; the type checker restricts it to `unsafe` blocks.
    if let Expr::Member { object, property } = callee
        && let Expr::Variable { name } = object.as_ref()
        && name.lexeme() == "memory"
        && property.lexeme() == "raw_address"
    {
        let argument = arguments.first().ok_or_else(|| {
            crate::CodegenError::LLVMError("memory.raw_address expects 1 argument".into())
        })?;
        let tv = emit_expression(fn_ctx, argument)?;
        return match &tv.ntsc_type {
            Ty::Ref(inner, mutable) => Ok(TypedValue::new(
                tv.value,
                Ty::RawPointer(inner.clone(), *mutable),
            )),
            Ty::Own(inner) => Ok(TypedValue::new(
                tv.value,
                Ty::RawPointer(inner.clone(), true),
            )),
            other => Err(crate::CodegenError::LLVMError(format!(
                "memory.raw_address expects a reference, got `{other}`"
            ))),
        };
    }

    if let Expr::Variable { name } = callee {
        let name_str = name.lexeme();

        // `wait_any(a, b)` / `wait_all(a, b)` — concurrent execution of two
        // inline async blocks. Each argument must be an `Expr::AsyncBlock`.
        // Handled BEFORE emit_call_arguments to avoid evaluating the blocks
        // as regular expressions.
        if name_str == "wait_any" || name_str == "wait_all" {
            if arguments.len() != 2 {
                return Err(crate::CodegenError::LLVMError(format!(
                    "{name_str} expects exactly 2 arguments"
                )));
            }
            let anon_blocks = fn_ctx.block_span_to_name.clone().unwrap_or_default();
            let mut handles = Vec::new();
            let mut poll_fns = Vec::new();
            for arg in &arguments {
                let anon_name = match arg {
                    Expr::AsyncBlock { span, .. } => {
                        anon_blocks.get(&span.start).ok_or_else(|| {
                            crate::CodegenError::LLVMError(
                                "internal: async block not found in block_span_to_name".into(),
                            )
                        })?
                    }
                    _ => {
                        return Err(crate::CodegenError::LLVMError(format!(
                            "{name_str} arguments must be inline async blocks"
                        )));
                    }
                };
                let struct_name = format!("ntsc_future_{anon_name}");
                let future_ty = fn_ctx.module.get_struct_type(&struct_name).ok_or_else(|| {
                    crate::CodegenError::LLVMError(format!(
                        "internal: async block future struct {struct_name} not declared"
                    ))
                })?;
                let future_size = future_ty.size_of().ok_or_else(|| {
                    crate::CodegenError::LLVMError(format!("internal: {struct_name} has no size"))
                })?;
                let future_ptr = fn_ctx
                    .builder
                    .build_alloca(future_ty, &format!("{name_str}_future"))?;
                let zero = fn_ctx.context.i8_type().const_zero();
                fn_ctx
                    .builder
                    .build_memset(future_ptr, 1, zero, future_size)?;
                let state_ptr =
                    fn_ctx
                        .builder
                        .build_struct_gep(future_ty, future_ptr, 0, "state_ptr")?;
                fn_ctx
                    .builder
                    .build_store(state_ptr, fn_ctx.context.i32_type().const_int(0, false))?;
                let poll_name = format!("ntsc_future_{anon_name}_poll");
                let poll_fn_val = fn_ctx.module.get_function(&poll_name).ok_or_else(|| {
                    crate::CodegenError::LLVMError(format!(
                        "internal: poll function {poll_name} not declared"
                    ))
                })?;
                let poll_ptr = poll_fn_val.as_global_value().as_pointer_value();
                let poll_i8 = fn_ctx.builder.build_pointer_cast(
                    poll_ptr,
                    fn_ctx.context.ptr_type(AddressSpace::default()),
                    &format!("{name_str}_poll_fn"),
                )?;
                let handle = fn_ctx.builder.build_ptr_to_int(
                    future_ptr,
                    fn_ctx.context.i64_type(),
                    &format!("{name_str}_handle"),
                )?;
                handles.push(handle);
                poll_fns.push(poll_i8);
            }
            let runtime_fn_name = if name_str == "wait_any" {
                "ntsc_async_wait_any"
            } else {
                "ntsc_async_wait_all"
            };
            let runtime_fn = fn_ctx.module.get_function(runtime_fn_name).ok_or_else(|| {
                crate::CodegenError::LLVMError(format!("{runtime_fn_name} not declared"))
            })?;
            let result_handle = fn_ctx.builder.build_call(
                runtime_fn,
                &[
                    poll_fns[0].into(),
                    handles[0].into(),
                    poll_fns[1].into(),
                    handles[1].into(),
                ],
                &format!("{name_str}_result"),
            )?;

            // A throw inside one of the branches sets the thread-local
            // pending-exception flag.  Inside async poll functions
            // `exception_checks` is off, so temporarily enable it to
            // propagate the exception to the enclosing try/catch.
            let saved_exc = fn_ctx.exception_checks;
            fn_ctx.exception_checks = true;
            fn_ctx.emit_pending_exception_check()?;
            fn_ctx.exception_checks = saved_exc;

            let winner_handle = call_result_to_value(fn_ctx, &result_handle).into_int_value();

            // Read the result from the winning future's result field.
            let winner_ptr = fn_ctx.builder.build_int_to_ptr(
                winner_handle,
                fn_ctx.context.ptr_type(AddressSpace::default()),
                "winner_ptr",
            )?;
            // Determine the result type from the first block's return type.
            let result_ty = match &arguments[0] {
                Expr::AsyncBlock {
                    return_type: Some(rt),
                    ..
                } => type_annotation_to_ty(&Some(rt.ty.clone())),
                _ => Ty::Void,
            };
            if result_ty == Ty::Void {
                return Ok(TypedValue::new(
                    fn_ctx.context.i8_type().const_zero().into(),
                    Ty::Void,
                ));
            }
            let struct_name = format!(
                "ntsc_future_{}",
                anon_blocks.get(&arguments[0].span().start).unwrap()
            );
            let future_ty = fn_ctx.module.get_struct_type(&struct_name).ok_or_else(|| {
                crate::CodegenError::LLVMError(format!(
                    "internal: future struct {struct_name} not declared"
                ))
            })?;
            let result_field =
                fn_ctx
                    .builder
                    .build_struct_gep(future_ty, winner_ptr, 1, "result_ptr")?;
            let result_val = fn_ctx.builder.build_load(
                ty_to_llvm(&result_ty, fn_ctx.context),
                result_field,
                "result_val",
            )?;
            return Ok(TypedValue::new(result_val, result_ty));
        }

        let arg_values = emit_call_arguments(fn_ctx, &arguments)?;

        // `alloc(value)` moves its argument into an owning allocation.
        if name_str == "alloc" {
            let value = arg_values
                .into_iter()
                .next()
                .ok_or_else(|| crate::CodegenError::LLVMError("alloc expects 1 argument".into()))?;
            return emit_box_value(fn_ctx, value);
        }

        // `Ok(v)` / `Err(e)` build a fresh result cell owning their payload;
        // a bare-variable payload moves in and its slot is nulled. A user
        // definition of the same name (e.g. an enum variant's constructor)
        // shadows the builtin and keeps the ordinary call flow.
        if matches!(name_str, "Ok" | "Err") && fn_ctx.module.get_function(name_str).is_none() {
            if arguments.len() != 1 {
                return Err(crate::CodegenError::LLVMError(format!(
                    "{name_str} expects 1 argument"
                )));
            }
            let want_ok = name_str == "Ok";
            let payload_ty = arg_values[0].ntsc_type.clone();
            let ok_ty = if want_ok { payload_ty.clone() } else { Ty::Any };
            let err_ty = if want_ok { Ty::String } else { payload_ty };
            let boxed = box_result_value(
                fn_ctx,
                &ok_ty,
                &err_ty,
                &arguments[0],
                &arg_values[0],
                want_ok,
            )?;
            if let Expr::Variable { name } = &arguments[0]
                && ty_is_owned_handle(&arg_values[0].ntsc_type)
            {
                fn_ctx.null_var_slot(name.lexeme());
            }
            emit_drop_borrowed_fresh_args(fn_ctx, &arguments, &arg_values, &[])?;
            return Ok(boxed);
        }

        if name_str == "say" {
            let converted = if arguments.is_empty() {
                return Err(crate::CodegenError::LLVMError(
                    "say expects 1 argument".into(),
                ));
            } else {
                convert_to_string(fn_ctx, &arg_values[0])?
            };

            let say_fn = fn_ctx
                .module
                .get_function("ntsc_say")
                .ok_or_else(|| crate::CodegenError::LLVMError("ntsc_say not declared".into()))?;

            let handle = converted.value.into_int_value();
            fn_ctx.builder.build_call(
                say_fn,
                &[BasicMetadataValueEnum::IntValue(handle)],
                "say_call",
            )?;

            emit_drop_borrowed_fresh_args(fn_ctx, &arguments, &arg_values, &[])?;

            return Ok(TypedValue::new(
                fn_ctx.context.i8_type().const_zero().into(),
                Ty::Void,
            ));
        }

        let var_fn_info = fn_ctx
            .lookup_var(name_str)
            .map(|(p, t)| (p, t.clone()))
            .filter(|(_, t)| matches!(t, Ty::Function { .. }));
        if let Some((_ptr, fn_ty)) = var_fn_info
            && let Ty::Function {
                params,
                return_type,
            } = fn_ty
            && let Some(fn_val) = fn_ctx.module.get_function(name_str)
        {
            let prepared = prepare_call_args(fn_ctx, &arguments, &arg_values, &params)?;
            let mut llvm_args: Vec<BasicMetadataValueEnum<'ctx>> = Vec::new();
            for (arg_val, param_ty) in prepared.iter().zip(params.iter()) {
                llvm_args.push(
                    coerce_value(fn_ctx, arg_val.clone(), param_ty)?
                        .value
                        .into(),
                );
            }

            let result = fn_ctx.builder.build_call(fn_val, &llvm_args, "calltmp")?;
            let ret_ty = *return_type;
            let result_val = call_result_to_value(fn_ctx, &result);
            emit_drop_borrowed_fresh_args(fn_ctx, &arguments, &arg_values, &params)?;
            fn_ctx.emit_pending_exception_check()?;

            return Ok(TypedValue::new(result_val, ret_ty));
        }

        if let Some(fn_val) = fn_ctx.module.get_function(name_str) {
            let param_tys = fn_val.get_type().get_param_types();
            let declared_param_tys = function_declared_param_types(name_str);

            let prepared = prepare_call_args(fn_ctx, &arguments, &arg_values, &declared_param_tys)?;
            let mut llvm_args: Vec<BasicMetadataValueEnum<'ctx>> = Vec::new();
            for (arg_val, param_ty) in prepared.iter().zip(param_tys.iter()) {
                llvm_args.push(coerce_value_to_llvm(fn_ctx, arg_val.clone(), param_ty)?);
            }

            let result = fn_ctx.builder.build_call(fn_val, &llvm_args, "calltmp")?;
            let result_val = call_result_to_value(fn_ctx, &result);
            emit_drop_borrowed_fresh_args(fn_ctx, &arguments, &arg_values, &declared_param_tys)?;

            let ret_ty = function_declared_ret_ty(name_str).unwrap_or_else(|| {
                fn_val
                    .get_type()
                    .get_return_type()
                    .map(llvm_ret_ty_to_ty)
                    .unwrap_or(Ty::Void)
            });
            fn_ctx.emit_pending_exception_check()?;

            return Ok(TypedValue::new(result_val, ret_ty));
        }

        let var_info = fn_ctx.lookup_var(name_str).map(|(p, t)| (p, t.clone()));
        if let Some((slot, fn_ty)) = var_info
            && matches!(fn_ty, Ty::Function { .. })
        {
            let loaded = fn_ctx.builder.build_load(
                ty_to_llvm(&fn_ty, fn_ctx.context),
                slot,
                "fnptr_load",
            )?;
            let fn_params = if let Ty::Function { params, .. } = &fn_ty {
                params.clone()
            } else {
                Vec::new()
            };

            let prepared = prepare_call_args(fn_ctx, &arguments, &arg_values, &fn_params)?;
            let fn_val =
                emit_function_pointer_call(fn_ctx, loaded.into_pointer_value(), &prepared, &fn_ty)?;
            emit_drop_borrowed_fresh_args(fn_ctx, &arguments, &arg_values, &fn_params)?;
            fn_ctx.emit_pending_exception_check()?;
            return Ok(fn_val);
        }

        let builtins = [
            "ntsc_print_i64",
            "ntsc_print_f64",
            "ntsc_bool_to_string",
            "ntsc_i64_to_string",
            "ntsc_f64_to_string",
        ];

        if builtins.contains(&name_str)
            && let Some(fn_val) = fn_ctx.module.get_function(name_str)
        {
            let mut llvm_args: Vec<BasicMetadataValueEnum<'ctx>> = Vec::new();
            for arg_val in &arg_values {
                llvm_args.push(arg_val.value.into());
            }
            let result = fn_ctx.builder.build_call(fn_val, &llvm_args, "calltmp")?;
            let ret_val = call_result_to_value(fn_ctx, &result);
            return Ok(TypedValue::new(ret_val, Ty::Any));
        }

        if let Some(struct_ty) = fn_ctx.module.get_struct_type(name_str) {
            let result =
                emit_class_constructor(fn_ctx, struct_ty, name_str, &arguments, &arg_values, None)?;

            fn_ctx.emit_pending_exception_check()?;
            return Ok(result);
        }

        return Err(crate::CodegenError::LLVMError(format!(
            "undefined function `{name_str}`"
        )));
    }

    if let Expr::Member { object, property } = callee {
        let prop_name = property.lexeme();

        if let Expr::Variable { name } = object.as_ref() {
            let module_name = name.lexeme();

            // `chan.new(capacity)` — create a virtual-task channel. The
            // element-ownership flag comes from the annotated slot type the
            // initializer is stored into (`chan[string]` owns its heap
            // elements, `chan[int]` stores raw scalars).
            if module_name == "chan" && prop_name == "new" {
                if arguments.len() != 1 {
                    return Err(crate::CodegenError::LLVMError(
                        "chan.new expects exactly 1 capacity argument".into(),
                    ));
                }
                let arg_values = emit_call_arguments(fn_ctx, &arguments)?;
                let capacity = arg_values
                    .first()
                    .ok_or_else(|| {
                        crate::CodegenError::LLVMError("chan.new expects a capacity".into())
                    })?
                    .value
                    .into_int_value();
                let element = match &fn_ctx.expected_ty {
                    Some(Ty::Chan(element)) => (**element).clone(),
                    _ => Ty::Any,
                };
                let owns_elements = if ty_is_owned_handle(&element) { 1 } else { 0 };
                let new_fn = fn_ctx
                    .module
                    .get_function("ntask_chan_new")
                    .ok_or_else(|| {
                        crate::CodegenError::LLVMError("ntask_chan_new not declared".into())
                    })?;
                let result = fn_ctx.builder.build_call(
                    new_fn,
                    &[
                        capacity.into(),
                        fn_ctx
                            .context
                            .i8_type()
                            .const_int(owns_elements, false)
                            .into(),
                    ],
                    "chan_new",
                )?;
                let handle = call_result_to_value(fn_ctx, &result);
                return Ok(TypedValue::new(handle, Ty::Chan(Box::new(element))));
            }

            // A stdlib alias (`use strings as s`) dispatches against the real
            // module name: native functions are `ntsc_strings_*` and the
            // routed opcodes key off the module name.
            let dispatch_module = super::STDLIB_ALIASES
                .with(|m| m.borrow().get(module_name).cloned())
                .unwrap_or_else(|| module_name.to_string());

            if dispatch_module == "arrays" {
                let mut arg_values = emit_call_arguments(fn_ctx, &arguments)?;

                if let Some(first) = arg_values.first().cloned()
                    && matches!(first.ntsc_type, Ty::Shared(_))
                {
                    arg_values[0] = deref_shared(fn_ctx, first)?;
                }
                let result = emit_rc_array_op(fn_ctx, prop_name, &arg_values)?;

                emit_drop_borrowed_fresh_args(fn_ctx, &arguments, &arg_values, &[])?;
                fn_ctx.emit_pending_exception_check()?;
                return Ok(result);
            }

            if matches!(dispatch_module.as_str(), "sort" | "testing")
                || (dispatch_module == "random" && matches!(prop_name, "shuffle" | "weighted"))
                || (dispatch_module == "process" && prop_name == "spawn_thread")
            {
                let mut arg_values = emit_call_arguments(fn_ctx, &arguments)?;
                if let Some(first) = arg_values.first().cloned()
                    && matches!(first.ntsc_type, Ty::Shared(_))
                {
                    arg_values[0] = deref_shared(fn_ctx, first)?;
                }
                let result =
                    emit_routed_module_op(fn_ctx, &dispatch_module, prop_name, &arg_values)?;
                emit_drop_borrowed_fresh_args(fn_ctx, &arguments, &arg_values, &[])?;
                fn_ctx.emit_pending_exception_check()?;
                return Ok(result);
            }

            // A file-import namespace (`use "file.nt" as arm` -> `arm.func()`)
            // dispatches to the module's own function, whose global name is
            // the namespaced `arm::func`. A namespaced class is constructed
            // through the alias as well (`arm.Counter(...)`).
            let namespaced_fn = format!("{module_name}::{prop_name}");
            if let Some(fn_val) = fn_ctx.module.get_function(&namespaced_fn) {
                let arg_values = emit_call_arguments(fn_ctx, &arguments)?;
                let param_tys = fn_val.get_type().get_param_types();
                let declared_param_tys = function_declared_param_types(&namespaced_fn);
                let prepared =
                    prepare_call_args(fn_ctx, &arguments, &arg_values, &declared_param_tys)?;
                let mut llvm_args: Vec<BasicMetadataValueEnum<'ctx>> = Vec::new();
                for (arg_val, param_ty) in prepared.iter().zip(param_tys.iter()) {
                    llvm_args.push(coerce_value_to_llvm(fn_ctx, arg_val.clone(), param_ty)?);
                }
                let result = fn_ctx
                    .builder
                    .build_call(fn_val, &llvm_args, "alias_call")?;
                let result_val = call_result_to_value(fn_ctx, &result);
                emit_drop_borrowed_fresh_args(
                    fn_ctx,
                    &arguments,
                    &arg_values,
                    &declared_param_tys,
                )?;
                let ret_ty = function_declared_ret_ty(&namespaced_fn).unwrap_or_else(|| {
                    fn_val
                        .get_type()
                        .get_return_type()
                        .map(llvm_ret_ty_to_ty)
                        .unwrap_or(Ty::Void)
                });
                fn_ctx.emit_pending_exception_check()?;
                return Ok(TypedValue::new(result_val, ret_ty));
            }
            if let Some(struct_ty) = fn_ctx.module.get_struct_type(&namespaced_fn) {
                let arg_values = emit_call_arguments(fn_ctx, &arguments)?;
                let result = emit_class_constructor(
                    fn_ctx,
                    struct_ty,
                    &namespaced_fn,
                    &arguments,
                    &arg_values,
                    None,
                )?;
                fn_ctx.emit_pending_exception_check()?;
                return Ok(result);
            }

            // A stdlib alias (`use strings as s`) was already resolved to the
            // real module name above; emit the native ABI call against it.
            let abi_fn_name = format!("ntsc_{dispatch_module}_{prop_name}");
            if let Some(fn_val) = fn_ctx.module.get_function(&abi_fn_name) {
                let arg_values = emit_call_arguments(fn_ctx, &arguments)?;

                let param_tys = fn_val.get_type().get_param_types();
                let mut llvm_args: Vec<BasicMetadataValueEnum<'ctx>> = Vec::new();
                for (arg_val, param_ty) in arg_values.iter().zip(param_tys.iter()) {
                    llvm_args.push(coerce_value_to_llvm(fn_ctx, arg_val.clone(), param_ty)?);
                }
                let result = fn_ctx.builder.build_call(fn_val, &llvm_args, "modcall")?;
                let mut ret_val = call_result_to_value(fn_ctx, &result);

                emit_drop_borrowed_fresh_args(fn_ctx, &arguments, &arg_values, &[])?;
                let ty = stdlib_return_ty(&abi_fn_name);

                if ty == Ty::Bool
                    && ret_val.is_int_value()
                    && ret_val.into_int_value().get_type().get_bit_width() == 8
                {
                    ret_val = fn_ctx
                        .builder
                        .build_int_truncate(
                            ret_val.into_int_value(),
                            fn_ctx.context.bool_type(),
                            "std_bool",
                        )?
                        .into();
                }
                fn_ctx.emit_pending_exception_check()?;
                return normalize_self(fn_ctx, TypedValue::new(ret_val, ty));
            }
        }

        let obj_val = emit_expression(fn_ctx, object)?;

        let obj_val = deref_shared(fn_ctx, obj_val)?;

        let arg_values = emit_call_arguments(fn_ctx, &arguments)?;

        // Result/option combinators (`r.map(f)`, `opt.ok_or(e)`, ...) are
        // builtins on those types; a class method with the same name wins
        // for class receivers because typeck only routes here otherwise.
        if let Some(result) = super::result_cell::emit_result_combinator(
            fn_ctx,
            object,
            &obj_val,
            prop_name,
            &arguments,
            &arg_values,
        )? {
            return Ok(result);
        }

        let dispatch_label = match &obj_val.ntsc_type {
            Ty::View(inner, _) => inner.label(),
            other => other.label(),
        };

        // A `dyn` receiver dispatches through its own vtable instead of a
        // statically known class method; `own dyn` boxes the same fat
        // pointer, so unbox first.
        let dyn_target = match &obj_val.ntsc_type {
            Ty::Dyn(_) => Some(obj_val.clone()),
            Ty::Own(inner) if matches!(**inner, Ty::Dyn(_)) => {
                let loaded = super::helper::load_own_cell(fn_ctx, &obj_val, inner)?;
                Some(TypedValue::new(loaded, (**inner).clone()))
            }
            _ => None,
        };
        if let Some(receiver) = dyn_target {
            return super::dyn_obj::emit_dyn_method_call(
                fn_ctx,
                receiver,
                property,
                &arguments,
                &arg_values,
            );
        }

        let Some(declaring) = class_method_declaring_class(&dispatch_label, prop_name) else {
            return Err(crate::CodegenError::LLVMError(format!(
                "undefined method `{}` on type `{}`",
                prop_name, obj_val.ntsc_type
            )));
        };
        let method_name = format!("{declaring}.{prop_name}");

        if let Some(fn_val) = fn_ctx.module.get_function(&method_name) {
            let method_param_tys = class_method_declared_param_types(&declaring, prop_name);

            let prepared = prepare_call_args(fn_ctx, &arguments, &arg_values, &method_param_tys)?;
            let receiver = if declaring == dispatch_label {
                obj_val.value.into_pointer_value()
            } else {
                fn_ctx.builder.build_pointer_cast(
                    obj_val.value.into_pointer_value(),
                    fn_ctx.context.ptr_type(AddressSpace::default()),
                    "method_receiver",
                )?
            };
            let mut llvm_args = vec![BasicMetadataValueEnum::PointerValue(receiver)];
            for arg_val in &prepared {
                llvm_args.push(arg_val.value.into());
            }

            let result = fn_ctx.builder.build_call(fn_val, &llvm_args, "methodtmp")?;
            let ret_val = call_result_to_value(fn_ctx, &result);
            emit_drop_borrowed_fresh_args(fn_ctx, &arguments, &arg_values, &method_param_tys)?;

            let ret_ty = class_method_ret_ty(&declaring, prop_name)
                .filter(|ty| *ty != Ty::Void)
                .or_else(|| fn_val.get_type().get_return_type().map(llvm_ret_ty_to_ty))
                .unwrap_or(Ty::Void);
            fn_ctx.emit_pending_exception_check()?;

            return Ok(TypedValue::new(ret_val, ret_ty));
        }

        return Err(crate::CodegenError::LLVMError(format!(
            "undefined method `{}` on type `{}`",
            prop_name, obj_val.ntsc_type
        )));
    }

    let callee_val = emit_expression(fn_ctx, callee)?;
    if matches!(callee_val.ntsc_type, Ty::Function { .. }) && callee_val.value.is_pointer_value() {
        let fn_ptr = callee_val.value.into_pointer_value();
        let arg_values = emit_call_arguments(fn_ctx, &arguments)?;
        let fn_params = if let Ty::Function { params, .. } = &callee_val.ntsc_type {
            params.clone()
        } else {
            Vec::new()
        };

        let prepared = prepare_call_args(fn_ctx, &arguments, &arg_values, &fn_params)?;
        let result = emit_function_pointer_call(fn_ctx, fn_ptr, &prepared, &callee_val.ntsc_type)?;
        emit_drop_borrowed_fresh_args(fn_ctx, &arguments, &arg_values, &fn_params)?;
        fn_ctx.emit_pending_exception_check()?;
        return Ok(result);
    }

    Err(crate::CodegenError::LLVMError(format!(
        "call through expression not supported for value of type `{}`",
        callee_val.ntsc_type
    )))
}

pub(crate) fn emit_function_pointer_call<'ctx>(
    fn_ctx: &mut FunctionContext<'ctx, '_>,
    fn_ptr: PointerValue<'ctx>,
    arg_values: &[TypedValue<'ctx>],
    fn_ty: &Ty,
) -> Result<TypedValue<'ctx>, crate::CodegenError> {
    let Ty::Function {
        params,
        return_type,
    } = fn_ty
    else {
        return Err(crate::CodegenError::LLVMError(format!(
            "cannot call a value of type `{fn_ty}`"
        )));
    };

    let param_tys: Vec<inkwell::types::BasicMetadataTypeEnum<'ctx>> = params
        .iter()
        .map(|p| ty_to_llvm(p, fn_ctx.context).into())
        .collect();
    let llvm_fn_ty = if **return_type == Ty::Void {
        fn_ctx.context.void_type().fn_type(&param_tys, false)
    } else {
        ty_to_llvm(return_type, fn_ctx.context).fn_type(&param_tys, false)
    };

    let mut llvm_args: Vec<BasicMetadataValueEnum<'ctx>> = Vec::new();
    for (arg, param_ty) in arg_values.iter().zip(params.iter()) {
        llvm_args.push(coerce_value(fn_ctx, arg.clone(), param_ty)?.value.into());
    }

    let result = fn_ctx
        .builder
        .build_indirect_call(llvm_fn_ty, fn_ptr, &llvm_args, "fptr_call")?;
    let result_val = call_result_to_value(fn_ctx, &result);
    Ok(TypedValue::new(result_val, (**return_type).clone()))
}
