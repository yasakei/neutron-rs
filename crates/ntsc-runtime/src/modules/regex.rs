//! NTSC standard library: `regex` module.
//! Text and pattern arguments are borrowed string handles; returned strings
//! are owned by the caller. Invalid patterns throw.

use crate::registry;

fn make_regex(pattern: &str) -> Result<regex::Regex, String> {
    regex::Regex::new(pattern).map_err(|e| format!("Invalid regex: {e}"))
}

/// Throw for a failed `regex.<name>` call; usable as the tail expression of
/// i64-returning shims because it yields the failure sentinel.
fn fail(fn_name: &str, e: String) -> i64 {
    super::throw_str(format!("regex.{fn_name}: {e}"))
}

#[unsafe(no_mangle)]
pub extern "C" fn ntsc_regex_test(text: i64, pattern: i64) -> i8 {
    let text = registry::get_string(text).unwrap_or_default();
    let pattern = registry::get_string(pattern).unwrap_or_default();
    match make_regex(&pattern) {
        Ok(re) => i8::from(re.is_match(&text)),
        Err(e) => {
            let _ = fail("test", e);
            0
        }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn ntsc_regex_search(text: i64, pattern: i64) -> i8 {
    ntsc_regex_test(text, pattern)
}

/// `regex.find(text, pattern)` — JSON with `matched`, `position`, `length`,
/// or "null" when nothing matches.
#[unsafe(no_mangle)]
pub extern "C" fn ntsc_regex_find(text: i64, pattern: i64) -> i64 {
    let text = registry::get_string(text).unwrap_or_default();
    let pat = registry::get_string(pattern).unwrap_or_default();
    match make_regex(&pat) {
        Ok(re) => match re.find(&text) {
            Some(m) => registry::put_string(format!(
                "{{\"matched\":\"{}\",\"position\":{},\"length\":{}}}",
                m.as_str().replace('"', "\\\""),
                m.start(),
                m.len()
            )),
            None => registry::put_string("null".to_string()),
        },
        Err(e) => fail("find", e),
    }
}

/// `regex.find_all(text, pattern)` — JSON array of match objects.
#[unsafe(no_mangle)]
pub extern "C" fn ntsc_regex_find_all(text: i64, pattern: i64) -> i64 {
    let text = registry::get_string(text).unwrap_or_default();
    let pat = registry::get_string(pattern).unwrap_or_default();
    match make_regex(&pat) {
        Ok(re) => {
            let matches: Vec<String> = re
                .find_iter(&text)
                .map(|m| {
                    format!(
                        "{{\"matched\":\"{}\",\"position\":{},\"length\":{}}}",
                        m.as_str().replace('"', "\\\""),
                        m.start(),
                        m.len()
                    )
                })
                .collect();
            registry::put_string(format!("[{}]", matches.join(",")))
        }
        Err(e) => fail("find_all", e),
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn ntsc_regex_replace(text: i64, pattern: i64, replacement: i64) -> i64 {
    let text = registry::get_string(text).unwrap_or_default();
    let pat = registry::get_string(pattern).unwrap_or_default();
    let repl = registry::get_string(replacement).unwrap_or_default();
    match make_regex(&pat) {
        Ok(re) => registry::put_string(re.replace_all(&text, &repl).to_string()),
        Err(e) => fail("replace", e),
    }
}

/// `regex.split(text, pattern)` — the parts, joined by newlines.
#[unsafe(no_mangle)]
pub extern "C" fn ntsc_regex_split(text: i64, pattern: i64) -> i64 {
    let text = registry::get_string(text).unwrap_or_default();
    let pat = registry::get_string(pattern).unwrap_or_default();
    match make_regex(&pat) {
        Ok(re) => {
            let parts: Vec<&str> = re.split(&text).collect();
            registry::put_string(parts.join("\n"))
        }
        Err(e) => fail("split", e),
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn ntsc_regex_is_valid(pattern: i64) -> i8 {
    let pat = registry::get_string(pattern).unwrap_or_default();
    i8::from(regex::Regex::new(&pat).is_ok())
}

#[unsafe(no_mangle)]
pub extern "C" fn ntsc_regex_escape(text: i64) -> i64 {
    let text = registry::get_string(text).unwrap_or_default();
    registry::put_string(regex::escape(&text))
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
    fn test_test() {
        assert_eq!(ntsc_regex_test(put("hello"), put("^h.*o$")), 1);
        assert_eq!(ntsc_regex_test(put("hi"), put("^h.*o$")), 0);
    }

    #[test]
    fn test_replace() {
        let r = ntsc_regex_replace(put("hello world"), put("world"), put("there"));
        assert_eq!(read(r), "hello there");
    }

    #[test]
    fn test_is_valid() {
        assert_eq!(ntsc_regex_is_valid(put("[a-z]+")), 1);
        assert_eq!(ntsc_regex_is_valid(put("[invalid")), 0);
    }

    #[test]
    fn test_split() {
        let r = ntsc_regex_split(put("a,b,c"), put(","));
        assert!(read(r).contains("a"));
    }

    #[test]
    fn test_invalid_pattern_throws() {
        use crate::modules::test_util::catch_throw;
        let err = catch_throw(|| {
            let _ = ntsc_regex_test(put("hi"), put("[invalid"));
        });
        let msg = err.unwrap();
        assert!(msg.contains("regex.test"), "unexpected message: {msg}");

        let err = catch_throw(|| {
            let _ = ntsc_regex_find(put("hi"), put("[invalid"));
        });
        let msg = err.unwrap();
        assert!(msg.contains("regex.find"), "unexpected message: {msg}");
    }
}
