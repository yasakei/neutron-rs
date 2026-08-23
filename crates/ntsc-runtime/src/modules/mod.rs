//! NTSC standard library: module registry.
//! Stdlib modules are compiled into `libntsc_runtime.a`; `ntsc-codegen`
//! forward-declares their extern "C" functions and the linker resolves them.

pub mod arrays;
pub mod collections;
pub mod crypto;
pub mod encoding;
pub mod fmt;
pub mod hash;
pub mod http;
pub mod io;
pub mod json;
pub mod math;
pub mod memory;
pub mod net;
pub mod os;
pub mod process;
pub mod random;
pub mod regex;
pub mod slices;
pub mod sort;
pub mod strings;
pub mod sys;
pub mod testing;
pub mod time;

use crate::registry;

/// Register `msg` as the pending exception and return 0, the failure
/// sentinel checked by generated callers.
pub(crate) fn throw_str(msg: String) -> i64 {
    crate::ntsc_throw(registry::put_string(msg))
}

#[cfg(test)]
pub(crate) mod test_util {
    use crate::registry;

    pub fn catch_throw(f: impl FnOnce()) -> Option<String> {
        f();
        if crate::ntsc_exception_pending() != 0 {
            let message = crate::ntsc_exception_take_message();
            let text = registry::get_string(message).unwrap_or_default();
            let _ = registry::take_string(message);
            Some(text)
        } else {
            None
        }
    }
}
