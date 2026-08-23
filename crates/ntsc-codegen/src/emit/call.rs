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
        | "ntsc_sys_args" => Ty::String,
        "ntsc_sys_write" | "ntsc_sys_append" | "ntsc_sys_exists" | "ntsc_sys_mkdir"
        | "ntsc_sys_cp" | "ntsc_sys_rm" => Ty::Bool,
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

        "ntsc_json_parse"
        | "ntsc_json_stringify"
        | "ntsc_json_get"
        | "ntsc_json_keys"
        | "ntsc_json_stringify_pretty" => Ty::String,
        "ntsc_json_is_valid" | "ntsc_json_has" => Ty::Bool,

        "ntsc_http_get" | "ntsc_http_post" | "ntsc_http_put" | "ntsc_http_delete"
        | "ntsc_http_head" | "ntsc_http_patch" | "ntsc_http_request" => Ty::String,
        "ntsc_http_status_code" => Ty::Int,

        "ntsc_crypto_base64_encode"
        | "ntsc_crypto_base64_decode"
        | "ntsc_crypto_sha256"
        | "ntsc_crypto_hex_encode"
        | "ntsc_crypto_hex_decode"
        | "ntsc_crypto_random_bytes"
        | "ntsc_crypto_random_string"
        | "ntsc_crypto_xor_cipher" => Ty::String,

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
        "ntsc_os_setenv" | "ntsc_os_unsetenv" | "ntsc_os_has_env" | "ntsc_os_is_abs" => Ty::Bool,

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

        let arg_values = emit_call_arguments(fn_ctx, &arguments)?;

        // `alloc(value)` moves its argument into an owning allocation.
        if name_str == "alloc" {
            let value = arg_values
                .into_iter()
                .next()
                .ok_or_else(|| crate::CodegenError::LLVMError("alloc expects 1 argument".into()))?;
            return emit_box_value(fn_ctx, value);
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

            if module_name == "arrays" {
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

            if matches!(module_name, "sort" | "testing")
                || (module_name == "random" && matches!(prop_name, "shuffle" | "weighted"))
                || (module_name == "process" && prop_name == "spawn_thread")
            {
                let mut arg_values = emit_call_arguments(fn_ctx, &arguments)?;
                if let Some(first) = arg_values.first().cloned()
                    && matches!(first.ntsc_type, Ty::Shared(_))
                {
                    arg_values[0] = deref_shared(fn_ctx, first)?;
                }
                let result = emit_routed_module_op(fn_ctx, module_name, prop_name, &arg_values)?;
                emit_drop_borrowed_fresh_args(fn_ctx, &arguments, &arg_values, &[])?;
                fn_ctx.emit_pending_exception_check()?;
                return Ok(result);
            }

            let abi_fn_name = format!("ntsc_{module_name}_{prop_name}");
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
