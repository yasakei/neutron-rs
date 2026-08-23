//! NTSC standard library: `json` module.
//! JSON values are compact JSON strings; arguments are borrowed handles,
//! returned handles are owned by the caller.

use crate::registry;

struct JsonParser<'a> {
    input: &'a str,
    pos: usize,
}

impl<'a> JsonParser<'a> {
    fn new(input: &'a str) -> Self {
        Self { input, pos: 0 }
    }

    fn skip_whitespace(&mut self) {
        while self.pos < self.input.len() && self.input.as_bytes()[self.pos].is_ascii_whitespace() {
            self.pos += 1;
        }
    }

    fn peek(&self) -> Option<u8> {
        self.input.as_bytes().get(self.pos).copied()
    }

    fn advance(&mut self) -> Option<u8> {
        let c = self.input.as_bytes().get(self.pos).copied();
        if c.is_some() {
            self.pos += 1;
        }
        c
    }

    fn expect(&mut self, expected: u8) -> Result<(), String> {
        if self.advance() == Some(expected) {
            Ok(())
        } else {
            Err(format!("Expected '{}'", expected as char))
        }
    }

    fn parse_value(&mut self) -> Result<String, String> {
        self.skip_whitespace();
        match self.peek() {
            Some(b'"') => self.parse_string(),
            Some(b'{') => self.parse_object(),
            Some(b'[') => self.parse_array(),
            Some(b't') | Some(b'f') => self.parse_bool(),
            Some(b'n') => self.parse_null(),
            Some(b'-') | Some(b'0'..=b'9') => self.parse_number(),
            Some(c) => Err(format!("Unexpected character: {}", c as char)),
            None => Err("Unexpected end of JSON".to_string()),
        }
    }

    fn parse_string(&mut self) -> Result<String, String> {
        self.expect(b'"')?;
        let mut result = String::new();
        loop {
            match self.advance() {
                Some(b'"') => return Ok(format!("\"{}\"", result)),
                Some(b'\\') => match self.advance() {
                    Some(b'"') => result.push('"'),
                    Some(b'\\') => result.push('\\'),
                    Some(b'/') => result.push('/'),
                    Some(b'b') => result.push('\u{0008}'),
                    Some(b'f') => result.push('\u{000C}'),
                    Some(b'n') => result.push('\n'),
                    Some(b'r') => result.push('\r'),
                    Some(b't') => result.push('\t'),
                    Some(b'u') => {
                        // \uXXXX escapes are not decoded; each is replaced with '?'.
                        let mut hex = String::new();
                        for _ in 0..4 {
                            if let Some(c) = self.advance() {
                                hex.push(c as char);
                            }
                        }
                        result.push('?');
                    }
                    Some(c) => result.push(c as char),
                    None => return Err("Unexpected end in string escape".to_string()),
                },
                Some(c) => result.push(c as char),
                None => return Err("Unterminated string".to_string()),
            }
        }
    }

    fn parse_number(&mut self) -> Result<String, String> {
        let start = self.pos;
        if self.peek() == Some(b'-') {
            self.advance();
        }
        while let Some(c) = self.peek() {
            // Allow '+'/'-' only immediately after 'e'/'E'.
            if c.is_ascii_digit() || c == b'.' || c == b'e' || c == b'E' || c == b'+' || c == b'-' {
                if (c == b'+' || c == b'-')
                    && (self.pos == start
                        || (self.pos > 0
                            && self.input.as_bytes()[self.pos - 1] != b'e'
                            && self.input.as_bytes()[self.pos - 1] != b'E'))
                {
                    break;
                }
                self.advance();
            } else {
                break;
            }
        }
        Ok(self.input[start..self.pos].to_string())
    }

    fn parse_bool(&mut self) -> Result<String, String> {
        if self.input[self.pos..].starts_with("true") {
            self.pos += 4;
            Ok("true".to_string())
        } else if self.input[self.pos..].starts_with("false") {
            self.pos += 5;
            Ok("false".to_string())
        } else {
            Err("Expected boolean".to_string())
        }
    }

    fn parse_null(&mut self) -> Result<String, String> {
        if self.input[self.pos..].starts_with("null") {
            self.pos += 4;
            Ok("null".to_string())
        } else {
            Err("Expected null".to_string())
        }
    }

    fn parse_array(&mut self) -> Result<String, String> {
        self.expect(b'[')?;
        let mut elements: Vec<String> = Vec::new();
        self.skip_whitespace();
        if self.peek() == Some(b']') {
            self.advance();
            return Ok("[]".to_string());
        }
        loop {
            let val = self.parse_value()?;
            elements.push(val);
            self.skip_whitespace();
            match self.peek() {
                Some(b']') => {
                    self.advance();
                    return Ok(format!("[{}]", elements.join(",")));
                }
                Some(b',') => {
                    self.advance();
                    self.skip_whitespace();
                }
                _ => return Err("Expected ',' or ']' in array".to_string()),
            }
        }
    }

    fn parse_object(&mut self) -> Result<String, String> {
        self.expect(b'{')?;
        let mut pairs: Vec<String> = Vec::new();
        self.skip_whitespace();
        if self.peek() == Some(b'}') {
            self.advance();
            return Ok("{}".to_string());
        }
        loop {
            let key = self.parse_string()?;
            self.skip_whitespace();
            self.expect(b':')?;
            self.skip_whitespace();
            let val = self.parse_value()?;
            pairs.push(format!("{}:{}", key, val));
            self.skip_whitespace();
            match self.peek() {
                Some(b'}') => {
                    self.advance();
                    return Ok(format!("{{{}}}", pairs.join(",")));
                }
                Some(b',') => {
                    self.advance();
                    self.skip_whitespace();
                }
                _ => return Err("Expected ',' or '}' in object".to_string()),
            }
        }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn ntsc_json_parse(s: i64) -> i64 {
    let s = registry::get_string(s).unwrap_or_default();
    let mut parser = JsonParser::new(&s);
    match parser.parse_value() {
        Ok(result) => registry::put_string(result),
        Err(e) => super::throw_str(format!("json.parse: JSON parse error: {e}")),
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn ntsc_json_stringify(s: i64) -> i64 {
    let s = registry::get_string(s).unwrap_or_default();
    registry::put_string(s)
}

#[unsafe(no_mangle)]
pub extern "C" fn ntsc_json_is_valid(s: i64) -> i8 {
    let s = registry::get_string(s).unwrap_or_default();
    let mut parser = JsonParser::new(&s);
    i8::from(parser.parse_value().is_ok())
}

/// `json.get(json_str, key)` — the value for `key`, or the string "null"
/// when the key is absent (a real JSON `null` is indistinguishable).
#[unsafe(no_mangle)]
pub extern "C" fn ntsc_json_get(json_str: i64, key: i64) -> i64 {
    let json = registry::get_string(json_str).unwrap_or_default();
    let key = registry::get_string(key).unwrap_or_default();
    let search = format!("\"{key}\":");
    if let Some(pos) = json.find(&search) {
        let value_start = pos + search.len();
        let remaining = &json[value_start..];

        let mut parser = JsonParser::new(remaining);
        match parser.parse_value() {
            Ok(val) => return registry::put_string(val),
            Err(_) => return registry::put_string("null".to_string()),
        }
    }
    registry::put_string("null".to_string())
}

#[unsafe(no_mangle)]
pub extern "C" fn ntsc_json_has(json_str: i64, key: i64) -> i8 {
    let json = registry::get_string(json_str).unwrap_or_default();
    let key = registry::get_string(key).unwrap_or_default();
    let search = format!("\"{key}\":");
    i8::from(json.contains(&search))
}

/// `json.keys(json_str)` — comma-separated list of keys.
#[unsafe(no_mangle)]
pub extern "C" fn ntsc_json_keys(json_str: i64) -> i64 {
    let json = registry::get_string(json_str).unwrap_or_default();
    let mut keys: Vec<String> = Vec::new();
    let mut pos = 0;
    while let Some(start) = json[pos..].find('"') {
        let abs_start = pos + start + 1;
        if let Some(end) = json[abs_start..].find('"') {
            let key = &json[abs_start..abs_start + end];

            let after = abs_start + end + 1;
            if json.as_bytes().get(after) == Some(&b':') {
                keys.push(key.to_string());
                pos = after + 1;
            } else {
                pos = abs_start + end + 1;
            }
        } else {
            break;
        }
    }
    registry::put_string(keys.join(","))
}

#[unsafe(no_mangle)]
pub extern "C" fn ntsc_json_stringify_pretty(json_str: i64) -> i64 {
    let json = registry::get_string(json_str).unwrap_or_default();
    let mut result = String::new();
    let mut indent = 0;
    let mut in_string = false;
    for ch in json.chars() {
        if ch == '"' {
            in_string = !in_string;
            result.push(ch);
        } else if in_string {
            result.push(ch);
        } else {
            match ch {
                '{' | '[' => {
                    result.push(ch);
                    result.push('\n');
                    indent += 1;
                    for _ in 0..indent {
                        result.push_str("  ");
                    }
                }
                '}' | ']' => {
                    result.push('\n');
                    indent -= 1;
                    for _ in 0..indent {
                        result.push_str("  ");
                    }
                    result.push(ch);
                }
                ',' => {
                    result.push(ch);
                    result.push('\n');
                    for _ in 0..indent {
                        result.push_str("  ");
                    }
                }
                ':' => {
                    result.push_str(": ");
                }
                ' ' => {}
                _ => result.push(ch),
            }
        }
    }
    registry::put_string(result)
}

/// Escape a raw string as a JSON string literal (surrounding quotes and all),
/// for building object literals at runtime.
#[unsafe(no_mangle)]
pub extern "C" fn ntsc_json_escape_string(s: i64) -> i64 {
    let input = registry::get_string(s).unwrap_or_default();
    let mut escaped = String::from("\"");
    for ch in input.chars() {
        match ch {
            '"' => escaped.push_str("\\\""),
            '\\' => escaped.push_str("\\\\"),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            '\u{0008}' => escaped.push_str("\\b"),
            '\u{000C}' => escaped.push_str("\\f"),
            c => escaped.push(c),
        }
    }
    escaped.push('"');
    registry::put_string(escaped)
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
    fn test_parse_valid() {
        let r = ntsc_json_parse(put(r#"{"a":1,"b":"hello"}"#));
        let s = read(r);
        assert!(s.contains("\"a\":1"));
        assert!(s.contains("\"b\":\"hello\""));
    }

    #[test]
    fn test_parse_array() {
        let r = ntsc_json_parse(put("[1,2,3]"));
        assert_eq!(read(r), "[1,2,3]");
    }

    #[test]
    fn test_is_valid() {
        assert_eq!(ntsc_json_is_valid(put(r#"{"a":1}"#)), 1);
        assert_eq!(ntsc_json_is_valid(put("not json")), 0);
    }

    #[test]
    fn test_get() {
        let r = ntsc_json_get(put(r#"{"name":"Alice","age":30}"#), put("name"));
        assert_eq!(read(r), "\"Alice\"");
    }

    #[test]
    fn test_keys() {
        let r = ntsc_json_keys(put(r#"{"a":1,"b":2}"#));
        let s = read(r);
        assert!(s.contains("a"));
        assert!(s.contains("b"));
    }

    #[test]
    fn test_escape_string() {
        assert_eq!(
            read(ntsc_json_escape_string(put("a\"b\n"))),
            "\"a\\\"b\\n\""
        );
    }

    #[test]
    fn test_parse_invalid_throws() {
        use crate::modules::test_util::catch_throw;
        let input = put("not json");
        let err = catch_throw(|| {
            let _ = ntsc_json_parse(input);
        });
        let _ = registry::take_string(input);
        let msg = err.unwrap();
        assert!(msg.contains("json.parse"), "unexpected message: {msg}");
    }
}
