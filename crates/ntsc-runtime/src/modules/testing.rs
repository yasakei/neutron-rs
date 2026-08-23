//! NTSC standard library: `testing` module.
//! Assertions for test files; failures throw so `try`/`catch` can report
//! them.

use crate::registry;

fn fail(fn_name: &str, msg: impl std::fmt::Display) -> i64 {
    super::throw_str(format!("testing.{fn_name}: {msg}"))
}

#[unsafe(no_mangle)]
pub extern "C" fn ntsc_testing_assert_true(cond: i8) -> i8 {
    if cond == 0 {
        let _ = fail("assert_true", "expected true, got false");
        return 0;
    }
    1
}

#[unsafe(no_mangle)]
pub extern "C" fn ntsc_testing_assert_false(cond: i8) -> i8 {
    if cond != 0 {
        let _ = fail("assert_false", "expected false, got true");
        return 0;
    }
    1
}

#[unsafe(no_mangle)]
pub extern "C" fn ntsc_testing_assert_eq_int(a: i64, b: i64) -> i8 {
    if a != b {
        let _ = fail("assert_eq", format!("expected {a}, got {b}"));
        return 0;
    }
    1
}

#[unsafe(no_mangle)]
pub extern "C" fn ntsc_testing_assert_ne_int(a: i64, b: i64) -> i8 {
    if a == b {
        let _ = fail("assert_ne", format!("expected a value other than {a}"));
        return 0;
    }
    1
}

#[unsafe(no_mangle)]
pub extern "C" fn ntsc_testing_assert_eq_float(a: f64, b: f64) -> i8 {
    if a != b {
        let _ = fail("assert_eq", format!("expected {a}, got {b}"));
        return 0;
    }
    1
}

#[unsafe(no_mangle)]
pub extern "C" fn ntsc_testing_assert_ne_float(a: f64, b: f64) -> i8 {
    if a == b {
        let _ = fail("assert_ne", format!("expected a value other than {a}"));
        return 0;
    }
    1
}

#[unsafe(no_mangle)]
pub extern "C" fn ntsc_testing_assert_eq_bool(a: i8, b: i8) -> i8 {
    if a != b {
        let _ = fail("assert_eq", format!("expected {a}, got {b}"));
        return 0;
    }
    1
}

#[unsafe(no_mangle)]
pub extern "C" fn ntsc_testing_assert_ne_bool(a: i8, b: i8) -> i8 {
    if a == b {
        let _ = fail("assert_ne", format!("expected a value other than {a}"));
        return 0;
    }
    1
}

#[unsafe(no_mangle)]
pub extern "C" fn ntsc_testing_assert_eq_string(a: i64, b: i64) -> i8 {
    let a = registry::get_string(a).unwrap_or_default();
    let b = registry::get_string(b).unwrap_or_default();
    if a != b {
        let _ = fail("assert_eq", format!("expected \"{a}\", got \"{b}\""));
        return 0;
    }
    1
}

#[unsafe(no_mangle)]
pub extern "C" fn ntsc_testing_assert_ne_string(a: i64, b: i64) -> i8 {
    let a = registry::get_string(a).unwrap_or_default();
    let b = registry::get_string(b).unwrap_or_default();
    if a == b {
        let _ = fail("assert_ne", format!("expected a value other than \"{a}\""));
        return 0;
    }
    1
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::test_util::catch_throw;

    fn put(s: &str) -> i64 {
        registry::put_string(s.to_string())
    }

    #[test]
    fn test_assert_true() {
        assert_eq!(ntsc_testing_assert_true(1), 1);
        let msg = catch_throw(|| {
            let _ = ntsc_testing_assert_true(0);
        })
        .unwrap();
        assert!(
            msg.contains("testing.assert_true"),
            "unexpected message: {msg}"
        );
    }

    #[test]
    fn test_assert_false() {
        assert_eq!(ntsc_testing_assert_false(0), 1);
        let msg = catch_throw(|| {
            let _ = ntsc_testing_assert_false(1);
        })
        .unwrap();
        assert!(
            msg.contains("testing.assert_false"),
            "unexpected message: {msg}"
        );
    }

    #[test]
    fn test_assert_eq_int() {
        assert_eq!(ntsc_testing_assert_eq_int(1, 1), 1);
        let msg = catch_throw(|| {
            let _ = ntsc_testing_assert_eq_int(1, 2);
        })
        .unwrap();
        assert!(
            msg.contains("testing.assert_eq"),
            "unexpected message: {msg}"
        );
        assert!(
            msg.contains("1") && msg.contains("2"),
            "message lacks values: {msg}"
        );
    }

    #[test]
    fn test_assert_ne_int() {
        assert_eq!(ntsc_testing_assert_ne_int(1, 2), 1);
        let msg = catch_throw(|| {
            let _ = ntsc_testing_assert_ne_int(1, 1);
        })
        .unwrap();
        assert!(
            msg.contains("testing.assert_ne"),
            "unexpected message: {msg}"
        );
    }

    #[test]
    fn test_assert_eq_float() {
        assert_eq!(ntsc_testing_assert_eq_float(1.5, 1.5), 1);
        let msg = catch_throw(|| {
            let _ = ntsc_testing_assert_eq_float(1.5, 2.5);
        })
        .unwrap();
        assert!(
            msg.contains("testing.assert_eq"),
            "unexpected message: {msg}"
        );
    }

    #[test]
    fn test_assert_eq_string() {
        assert_eq!(ntsc_testing_assert_eq_string(put("a"), put("a")), 1);
        let msg = catch_throw(|| {
            let _ = ntsc_testing_assert_eq_string(put("a"), put("b"));
        })
        .unwrap();
        assert!(
            msg.contains("testing.assert_eq"),
            "unexpected message: {msg}"
        );
        assert!(
            msg.contains("a") && msg.contains("b"),
            "message lacks values: {msg}"
        );
    }

    #[test]
    fn test_assert_ne_string() {
        assert_eq!(ntsc_testing_assert_ne_string(put("a"), put("b")), 1);
        let msg = catch_throw(|| {
            let _ = ntsc_testing_assert_ne_string(put("a"), put("a"));
        })
        .unwrap();
        assert!(
            msg.contains("testing.assert_ne"),
            "unexpected message: {msg}"
        );
    }
}
