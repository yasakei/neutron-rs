//! The generated C `main` wrapper.

use super::*;

/// Wrap the user program as a real C `main`: call `__ntsc_user_main`,
/// shut the runtime down on every return path (reporting leaks in debug
/// builds), and convert the user return value to an `i32` exit code. A void
/// (or absent) user main exits 0.
pub(crate) fn emit_c_main_wrapper(
    module: &Module<'_>,
    report_leaks: bool,
) -> Result<(), crate::CodegenError> {
    let context = module.get_context();

    if let Some(user_main) = module.get_function("__ntsc_user_main") {
        let i32_type = context.i32_type();
        let c_main_type = i32_type.fn_type(&[], false);
        let c_main = module.add_function("main", c_main_type, None);
        let entry = context.append_basic_block(c_main, "entry");
        let builder = context.create_builder();
        builder.position_at_end(entry);

        let result = builder.build_call(user_main, &[], "")?;

        let shutdown_fn = module.get_function("ntsc_runtime_shutdown");
        let emit_shutdown = || -> Result<(), crate::CodegenError> {
            if let Some(shutdown) = shutdown_fn {
                let report = context.i8_type().const_int(u64::from(report_leaks), false);
                builder.build_call(
                    shutdown,
                    &[inkwell::values::BasicMetadataValueEnum::IntValue(report)],
                    "",
                )?;
            }
            Ok(())
        };

        match result.try_as_basic_value() {
            inkwell::values::ValueKind::Basic(inkwell::values::BasicValueEnum::IntValue(ret)) => {
                let truncated = builder.build_int_truncate(ret, i32_type, "exit_code")?;
                emit_shutdown()?;
                builder.build_return(Some(&truncated))?;
            }
            _ => {
                emit_shutdown()?;
                builder.build_return(Some(&i32_type.const_int(0, false)))?;
            }
        }
    }
    Ok(())
}
