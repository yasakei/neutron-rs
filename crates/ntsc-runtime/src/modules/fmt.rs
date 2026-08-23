//! NTSC standard library: `fmt` module.
//! Formatting, type checking, and conversion utilities.

use crate::registry;

/// Clamp a user-supplied pad width so the result stays below
/// [`MAX_PAD_OUTPUT`] bytes; `Vec` capacity overflows past `isize::MAX`.
fn clamped_pad_width(width: i64) -> usize {
    if width <= 0 {
        0
    } else {
        (width as usize).min(MAX_PAD_OUTPUT)
    }
}

/// Upper bound for `fmt.pad_*` output, in bytes.
const MAX_PAD_OUTPUT: usize = 1 << 24;

#[unsafe(no_mangle)]
pub extern "C" fn ntsc_fmt_to_int(s: i64) -> i64 {
    let text = match registry::get_string(s) {
        Some(t) => t.trim().to_string(),
        None => return super::throw_str("fmt.to_int: null input".to_string()),
    };
    match text.parse::<i64>() {
        Ok(v) => v,
        Err(_) => super::throw_str(format!("fmt.to_int: cannot parse '{text}' as an integer")),
    }
}

/// `fmt.to_float(str)` — throws when the string cannot be parsed as a float.
#[unsafe(no_mangle)]
pub extern "C" fn ntsc_fmt_to_float(s: i64) -> f64 {
    let text = match registry::get_string(s) {
        Some(t) => t.trim().to_string(),
        None => {
            super::throw_str("fmt.to_float: null input".to_string());
            return f64::NAN;
        }
    };
    match text.parse::<f64>() {
        Ok(v) => v,
        Err(_) => {
            super::throw_str(format!("fmt.to_float: cannot parse '{text}' as a float"));
            f64::NAN
        }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn ntsc_fmt_i64_to_str(val: i64) -> i64 {
    registry::put_string(format!("{}", val))
}

#[unsafe(no_mangle)]
pub extern "C" fn ntsc_fmt_f64_to_str(val: f64) -> i64 {
    registry::put_string(format!("{}", val))
}

/// `fmt.type_name(tag)` — tag: 0=nil, 1=bool, 2=int, 3=float, 4=string,
/// 5=array, 6=object.
#[unsafe(no_mangle)]
pub extern "C" fn ntsc_fmt_type_name(tag: i64) -> i64 {
    let name = match tag {
        0 => "nil",
        1 => "bool",
        2 => "int",
        3 => "float",
        4 => "string",
        5 => "array",
        6 => "object",
        _ => "unknown",
    };
    registry::put_string(name.to_string())
}

#[unsafe(no_mangle)]
pub extern "C" fn ntsc_fmt_is_int(s: i64) -> i8 {
    registry::get_string(s)
        .map(|t| i8::from(t.trim().parse::<i64>().is_ok()))
        .unwrap_or(0)
}

#[unsafe(no_mangle)]
pub extern "C" fn ntsc_fmt_is_float(s: i64) -> i8 {
    registry::get_string(s)
        .map(|t| i8::from(t.trim().parse::<f64>().is_ok()))
        .unwrap_or(0)
}

#[unsafe(no_mangle)]
pub extern "C" fn ntsc_fmt_to_hex(val: i64) -> i64 {
    registry::put_string(format!("{:x}", val))
}

#[unsafe(no_mangle)]
pub extern "C" fn ntsc_fmt_to_oct(val: i64) -> i64 {
    registry::put_string(format!("{:o}", val))
}

#[unsafe(no_mangle)]
pub extern "C" fn ntsc_fmt_pad_left(s: i64, width: i64, pad_char: u8) -> i64 {
    let s = registry::get_string(s).unwrap_or_default();
    let w = clamped_pad_width(width);
    if s.len() >= w {
        return registry::put_string(s);
    }
    let pad_count = w - s.len();
    let mut result = String::with_capacity(w);
    let ch = char::from(pad_char);
    for _ in 0..pad_count {
        result.push(ch);
    }
    result.push_str(&s);
    registry::put_string(result)
}

#[unsafe(no_mangle)]
pub extern "C" fn ntsc_fmt_pad_right(s: i64, width: i64, pad_char: u8) -> i64 {
    let s = registry::get_string(s).unwrap_or_default();
    let w = clamped_pad_width(width);
    if s.len() >= w {
        return registry::put_string(s);
    }
    let pad_count = w - s.len();
    let mut result = String::with_capacity(w);
    result.push_str(&s);
    let ch = char::from(pad_char);
    for _ in 0..pad_count {
        result.push(ch);
    }
    registry::put_string(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn put(s: &str) -> i64 {
        registry::put_string(s.to_string())
    }

    fn read(id: i64) -> String {
        let s = registry::get_string(id).unwrap_or_default();
        let _ = registry::take_string(id);
        s
    }

    #[test]
    fn test_to_int() {
        assert_eq!(ntsc_fmt_to_int(put("42")), 42);
        assert_eq!(ntsc_fmt_to_int(put("-7")), -7);
    }

    #[test]
    fn test_to_int_throws_on_parse_failure() {
        use crate::modules::test_util::catch_throw;
        let err = catch_throw(|| {
            let _ = ntsc_fmt_to_int(put("not a number"));
        });
        assert!(err.unwrap().contains("fmt.to_int"));

        let err = catch_throw(|| {
            let _ = ntsc_fmt_to_int(registry::NULL);
        });
        assert!(err.unwrap().contains("fmt.to_int"));
    }

    #[test]
    fn test_to_float_throws_on_parse_failure() {
        use crate::modules::test_util::catch_throw;
        let err = catch_throw(|| {
            let _ = ntsc_fmt_to_float(put("oops"));
        });
        assert!(err.unwrap().contains("fmt.to_float"));
    }

    #[test]
    fn test_type_name() {
        assert_eq!(read(ntsc_fmt_type_name(0)), "nil");
    }
}
