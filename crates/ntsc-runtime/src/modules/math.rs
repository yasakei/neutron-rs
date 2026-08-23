//! NTSC standard library: `math` module.

/// `math.sqrt(x)` — throws when `x` is negative (the f64 result is NaN).
#[unsafe(no_mangle)]
pub extern "C" fn ntsc_math_sqrt(x: f64) -> f64 {
    if x < 0.0 {
        let _ = super::throw_str(format!(
            "math.sqrt: cannot take the square root of a negative number ({x})"
        ));
        return f64::NAN;
    }
    x.sqrt()
}

#[unsafe(no_mangle)]
pub extern "C" fn ntsc_math_pow(base: f64, exp: f64) -> f64 {
    base.powf(exp)
}

#[unsafe(no_mangle)]
pub extern "C" fn ntsc_math_abs(x: f64) -> f64 {
    x.abs()
}

#[unsafe(no_mangle)]
pub extern "C" fn ntsc_math_ceil(x: f64) -> f64 {
    x.ceil()
}

#[unsafe(no_mangle)]
pub extern "C" fn ntsc_math_floor(x: f64) -> f64 {
    x.floor()
}

#[unsafe(no_mangle)]
pub extern "C" fn ntsc_math_round(x: f64) -> f64 {
    x.round()
}

/// `math.sin(x)` — sine of `x` in radians.
#[unsafe(no_mangle)]
pub extern "C" fn ntsc_math_sin(x: f64) -> f64 {
    x.sin()
}

/// `math.cos(x)` — cosine of `x` in radians.
#[unsafe(no_mangle)]
pub extern "C" fn ntsc_math_cos(x: f64) -> f64 {
    x.cos()
}

/// `math.tan(x)` — tangent of `x` in radians.
#[unsafe(no_mangle)]
pub extern "C" fn ntsc_math_tan(x: f64) -> f64 {
    x.tan()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sqrt() {
        let result = ntsc_math_sqrt(4.0);
        assert!((result - 2.0).abs() < 1e-10);
    }

    #[test]
    fn test_sqrt_negative_throws() {
        use crate::modules::test_util::catch_throw;
        let err = catch_throw(|| {
            let _ = ntsc_math_sqrt(-1.0);
        });
        assert!(err.unwrap().contains("math.sqrt"));
    }

    #[test]
    fn test_pow() {
        let result = ntsc_math_pow(2.0, 3.0);
        assert!((result - 8.0).abs() < 1e-10);
    }

    #[test]
    fn test_abs() {
        assert!((ntsc_math_abs(-5.0) - 5.0).abs() < 1e-10);
        assert!((ntsc_math_abs(3.0) - 3.0).abs() < 1e-10);
    }

    #[test]
    fn test_trig() {
        let s = ntsc_math_sin(0.0);
        assert!((s - 0.0).abs() < 1e-10);
        let c = ntsc_math_cos(0.0);
        assert!((c - 1.0).abs() < 1e-10);
    }
}
