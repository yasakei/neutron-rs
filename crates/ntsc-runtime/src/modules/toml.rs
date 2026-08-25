//! NTSC standard library: `toml` module.
//! TOML values are compact TOML strings; arguments are borrowed handles,
//! returned handles are owned by the caller.

use crate::registry;

use crate::modules::unescape;

struct TomlParser<'a> {
    input: &'a str,
    pos: usize,
}

impl<'a> TomlParser<'a> {
    fn new(input: &'a str) -> Self {
        Self { input, pos: 0 }
    }

    fn skip_whitespace_and_comments(&mut self) {
        loop {
            while self.pos < self.input.len()
                && self.input.as_bytes()[self.pos].is_ascii_whitespace()
            {
                self.pos += 1;
            }
            if self.pos < self.input.len() && self.input.as_bytes()[self.pos] == b'#' {
                while self.pos < self.input.len() && self.input.as_bytes()[self.pos] != b'\n' {
                    self.pos += 1;
                }
            } else {
                break;
            }
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

    fn parse_key(&mut self) -> Result<String, String> {
        self.skip_whitespace_and_comments();
        if self.peek() == Some(b'"') {
            self.parse_bare_string()
        } else {
            let start = self.pos;
            while let Some(c) = self.peek() {
                if c.is_ascii_alphanumeric() || c == b'_' || c == b'-' || c == b'.' {
                    self.pos += 1;
                } else {
                    break;
                }
            }
            if self.pos == start {
                return Err("expected key".to_string());
            }
            Ok(self.input[start..self.pos].to_string())
        }
    }

    fn parse_value(&mut self) -> Result<String, String> {
        self.skip_whitespace_and_comments();
        match self.peek() {
            Some(b'"') => self.parse_quoted_string(),
            Some(b'\'') => self.parse_literal_string(),
            Some(b'[') => self.parse_array(),
            Some(b'{') => self.parse_inline_table(),
            Some(b't') | Some(b'f') => self.parse_bool(),
            Some(b'-') | Some(b'0'..=b'9') => self.parse_number(),
            _ => Err(format!(
                "unexpected character: {:?}",
                self.peek().map(|c| c as char)
            )),
        }
    }

    fn parse_quoted_string(&mut self) -> Result<String, String> {
        self.advance();
        let mut result = String::new();
        loop {
            match self.advance() {
                Some(b'"') => {
                    let escaped = result.replace('\\', "\\\\").replace('"', "\\\"");
                    return Ok(format!("\"{escaped}\""));
                }
                Some(b'\\') => match self.advance() {
                    Some(b'"') => result.push('"'),
                    Some(b'\\') => result.push('\\'),
                    Some(b'n') => result.push('\n'),
                    Some(b'r') => result.push('\r'),
                    Some(b't') => result.push('\t'),
                    Some(c) => result.push(c as char),
                    None => return Err("unexpected end in string escape".to_string()),
                },
                Some(c) => result.push(c as char),
                None => return Err("unterminated string".to_string()),
            }
        }
    }

    fn parse_bare_string(&mut self) -> Result<String, String> {
        self.parse_quoted_string()
    }

    fn parse_literal_string(&mut self) -> Result<String, String> {
        self.advance();
        let start = self.pos;
        while let Some(c) = self.peek() {
            if c == b'\'' {
                let s = self.input[start..self.pos].to_string();
                self.advance();
                return Ok(format!("\"{s}\""));
            }
            self.pos += 1;
        }
        Err("unterminated literal string".to_string())
    }

    fn parse_number(&mut self) -> Result<String, String> {
        let start = self.pos;
        if self.peek() == Some(b'-') {
            self.advance();
        }
        while let Some(c) = self.peek() {
            if c.is_ascii_digit() || c == b'.' || c == b'e' || c == b'E' || c == b'+' || c == b'-' {
                if (c == b'+' || c == b'-')
                    && (self.pos == start
                        || (self.pos > 0
                            && self.input.as_bytes()[self.pos - 1] != b'e'
                            && self.input.as_bytes()[self.pos - 1] != b'E'))
                {
                    break;
                }
                self.pos += 1;
            } else {
                break;
            }
        }
        let text = self.input[start..self.pos].to_string();
        if text.contains('.') || text.contains('e') || text.contains('E') {
            let val: f64 = text.parse().map_err(|e| format!("invalid float: {e}"))?;
            Ok(format!("{val}"))
        } else {
            let val: i64 = text.parse().map_err(|e| format!("invalid int: {e}"))?;
            Ok(val.to_string())
        }
    }

    fn parse_bool(&mut self) -> Result<String, String> {
        if self.input[self.pos..].starts_with("true") {
            self.pos += 4;
            Ok("true".to_string())
        } else if self.input[self.pos..].starts_with("false") {
            self.pos += 5;
            Ok("false".to_string())
        } else {
            Err("expected boolean".to_string())
        }
    }

    fn parse_array(&mut self) -> Result<String, String> {
        self.advance();
        let mut elements = Vec::new();
        self.skip_whitespace_and_comments();
        if self.peek() == Some(b']') {
            self.advance();
            return Ok("[]".to_string());
        }
        loop {
            self.skip_whitespace_and_comments();
            if self.peek() == Some(b'#') {
                self.skip_whitespace_and_comments();
            }
            let val = self.parse_value()?;
            elements.push(val);
            self.skip_whitespace_and_comments();
            match self.peek() {
                Some(b']') => {
                    self.advance();
                    return Ok(format!("[{}]", elements.join(",")));
                }
                Some(b',') => {
                    self.advance();
                }
                _ => return Err("expected ',' or ']' in array".to_string()),
            }
        }
    }

    fn parse_inline_table(&mut self) -> Result<String, String> {
        self.advance();
        let mut pairs = Vec::new();
        self.skip_whitespace_and_comments();
        if self.peek() == Some(b'}') {
            self.advance();
            return Ok("{}".to_string());
        }
        loop {
            let key = self.parse_key()?;
            self.skip_whitespace_and_comments();
            if self.advance() != Some(b'=') {
                return Err("expected '=' in inline table".to_string());
            }
            let val = self.parse_value()?;
            pairs.push(format!("\"{key}\":{val}"));
            self.skip_whitespace_and_comments();
            match self.peek() {
                Some(b'}') => {
                    self.advance();
                    return Ok(format!("{{{}}}", pairs.join(",")));
                }
                Some(b',') => {
                    self.advance();
                }
                _ => return Err("expected ',' or '}}' in inline table".to_string()),
            }
        }
    }

    fn parse_toml(&mut self) -> Result<String, String> {
        let mut result = String::from("{");
        let mut first = true;
        let mut current_table: Vec<String> = Vec::new();

        while self.pos < self.input.len() {
            self.skip_whitespace_and_comments();
            if self.pos >= self.input.len() {
                break;
            }
            match self.peek() {
                Some(b'[') => {
                    self.advance();
                    if self.peek() == Some(b'[') {
                        self.advance();
                        let key = self.parse_array_of_tables_header()?;
                        self.skip_whitespace_and_comments();
                        if self.peek() == Some(b']') {
                            self.advance();
                        }
                        if self.peek() == Some(b']') {
                            self.advance();
                        }
                        current_table = key;
                    } else {
                        let key = self.parse_section_header()?;
                        self.skip_whitespace_and_comments();
                        if self.peek() == Some(b']') {
                            self.advance();
                        }
                        current_table = key;
                    }
                    continue;
                }
                Some(_) => {}
                None => break,
            }
            let key = self.parse_key()?;
            self.skip_whitespace_and_comments();
            if self.advance() != Some(b'=') {
                return Err("expected '='".to_string());
            }
            let val = self.parse_value()?;
            let full_key = if current_table.is_empty() {
                format!("\"{key}\"")
            } else {
                let prefix: Vec<String> =
                    current_table.iter().map(|k| format!("\"{k}\"")).collect();
                format!("{}.\"{}\"", prefix.join("."), key)
            };
            if !first {
                result.push(',');
            }
            result.push_str(&format!("{full_key}:{val}"));
            first = false;
            self.skip_whitespace_and_comments();
        }

        let mut nested = build_nested_json(&result)?;
        if nested.ends_with(',') {
            nested.pop();
        }
        nested.push('}');
        Ok(nested)
    }

    fn parse_section_header(&mut self) -> Result<Vec<String>, String> {
        self.skip_whitespace_and_comments();
        let mut keys = Vec::new();
        loop {
            let key = self.parse_key()?;
            keys.push(key);
            self.skip_whitespace_and_comments();
            if self.peek() == Some(b'.') {
                self.advance();
            } else {
                break;
            }
        }
        Ok(keys)
    }

    fn parse_array_of_tables_header(&mut self) -> Result<Vec<String>, String> {
        self.parse_section_header()
    }
}

fn build_nested_json(flat: &str) -> Result<String, String> {
    let flat = flat.trim();
    if flat.is_empty() {
        return Ok(String::from("{"));
    }
    let mut result = String::from("{");
    let entries = split_json_entries(flat)?;
    let mut root_keys: Vec<String> = Vec::new();
    let mut sections: Vec<(Vec<String>, Vec<String>)> = Vec::new();

    for entry in &entries {
        if let Some(colon_pos) = entry.find(':') {
            let key_part = &entry[..colon_pos];
            let val_part = &entry[colon_pos + 1..];
            let parts: Vec<String> = key_part
                .split('.')
                .map(|s| s.trim().trim_matches('"').to_string())
                .collect();
            if parts.len() == 1 {
                root_keys.push(entry.clone());
            } else {
                let section_key = parts[..parts.len() - 1].to_vec();
                let field_key = parts.last().unwrap().clone();
                let field_val = val_part.trim().to_string();
                let entry_str = format!("\"{field_key}\":{field_val}");
                if let Some(existing) = sections.iter_mut().find(|(k, _)| *k == section_key) {
                    existing.1.push(entry_str);
                } else {
                    sections.push((section_key, vec![entry_str]));
                }
            }
        }
    }

    let mut first = true;
    for entry in &root_keys {
        if !first {
            result.push(',');
        }
        result.push_str(entry);
        first = false;
    }

    for (section_key, fields) in &sections {
        if !first {
            result.push(',');
        }
        let mut nested_val = String::from("{");
        for (i, field) in fields.iter().enumerate() {
            if i > 0 {
                nested_val.push(',');
            }
            nested_val.push_str(field);
        }
        nested_val.push('}');
        let full_key: Vec<String> = section_key.iter().map(|k| format!("\"{k}\"")).collect();
        result.push_str(&format!("{}.{}", full_key.join("."), nested_val));
        first = false;
    }

    Ok(result)
}

fn find_json_key_colon(entry: &str) -> Option<usize> {
    let bytes = entry.as_bytes();
    let len = bytes.len();
    let mut i = 0;
    while i < len {
        if bytes[i] == b'"' {
            i += 1;
            while i < len && bytes[i] != b'"' {
                if bytes[i] == b'\\' && i + 1 < len {
                    i += 2;
                } else {
                    i += 1;
                }
            }
            if i < len {
                i += 1;
            }
            while i < len && bytes[i] != b':' {
                i += 1;
            }
            if i < len {
                return Some(i);
            }
            return None;
        }
        i += 1;
    }
    None
}

fn split_json_entries(s: &str) -> Result<Vec<String>, String> {
    let mut entries = Vec::new();
    let mut current = String::new();
    let mut depth = 0;
    let mut in_str = false;
    let mut escape_next = false;
    for c in s.chars() {
        if escape_next {
            current.push(c);
            escape_next = false;
            continue;
        }
        if c == '\\' && in_str {
            current.push(c);
            escape_next = true;
            continue;
        }
        if c == '"' {
            in_str = !in_str;
            current.push(c);
            continue;
        }
        if in_str {
            current.push(c);
            continue;
        }
        if c == '{' || c == '[' {
            depth += 1;
            current.push(c);
        } else if c == '}' || c == ']' {
            depth -= 1;
            current.push(c);
        } else if c == ',' && depth == 0 {
            let trimmed = current.trim().to_string();
            if !trimmed.is_empty() {
                entries.push(trimmed);
            }
            current.clear();
        } else {
            current.push(c);
        }
    }
    let trimmed = current.trim().to_string();
    if !trimmed.is_empty() {
        entries.push(trimmed);
    }
    Ok(entries)
}

#[unsafe(no_mangle)]
pub extern "C" fn ntsc_toml_parse(s: i64) -> i64 {
    let input = registry::get_string(s).unwrap_or_default();
    let input = unescape(&input);
    let mut parser = TomlParser::new(&input);
    match parser.parse_toml() {
        Ok(result) => registry::put_string(result),
        Err(e) => super::throw_str(format!("toml.parse: TOML parse error: {e}")),
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn ntsc_toml_stringify(s: i64) -> i64 {
    let json = registry::get_string(s).unwrap_or_default();
    let json = json.trim();
    if json.is_empty() || json == "{}" {
        return registry::put_string(String::new());
    }
    if !json.starts_with('{') {
        return super::throw_str("toml.stringify: expected a JSON object".to_string());
    }
    let mut result = String::new();
    let mut sections: Vec<(String, String)> = Vec::new();
    let inner = &json[1..json.len() - 1].trim();
    if inner.is_empty() {
        return registry::put_string(String::new());
    }
    let entries = match split_json_entries(inner) {
        Ok(e) => e,
        Err(_) => {
            return super::throw_str("toml.stringify: malformed JSON object".to_string());
        }
    };
    for entry in &entries {
        if let Some(colon_pos) = find_json_key_colon(entry) {
            let key_part = &entry[..colon_pos].trim().trim_matches('"');
            let val_part = entry[colon_pos + 1..].trim();
            if val_part.starts_with('{') {
                sections.push((key_part.to_string(), val_part.to_string()));
            } else {
                result.push_str(&format!("{key_part} = {}\n", json_to_toml_value(val_part)));
            }
        }
    }
    for (key, val) in &sections {
        result.push_str(&format!("\n[{key}]\n"));
        if let Ok(nested_entries) = split_json_entries(val) {
            for entry in &nested_entries {
                if let Some(colon_pos) = find_json_key_colon(entry) {
                    let k = &entry[..colon_pos].trim().trim_matches('"');
                    let v = entry[colon_pos + 1..].trim();
                    result.push_str(&format!("{k} = {}\n", json_to_toml_value(v)));
                }
            }
        }
    }
    registry::put_string(result)
}

fn json_to_toml_value(val: &str) -> String {
    if (val.starts_with('"') && val.ends_with('"') && val.len() >= 2)
        || val == "true"
        || val == "false"
    {
        val.to_string()
    } else if val == "null" {
        "null".to_string()
    } else if val.starts_with('[') && val.ends_with(']') {
        let inner = &val[1..val.len() - 1].trim();
        if inner.is_empty() {
            "[]".to_string()
        } else {
            let items: Vec<&str> = inner.split(',').map(|s| s.trim()).collect();
            let formatted: Vec<String> = items.iter().map(|s| json_to_toml_value(s)).collect();
            format!("[{}]", formatted.join(", "))
        }
    } else {
        val.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::test_util::catch_throw;

    fn put(s: &str) -> i64 {
        registry::put_string(s.to_string())
    }

    fn read(id: i64) -> String {
        let s = registry::get_string(id).unwrap_or_default();
        let _ = registry::take_string(id);
        s
    }

    #[test]
    fn test_parse_simple() {
        let input = "name = \"Alice\"\nage = 30\nactive = true";
        let r = ntsc_toml_parse(put(input));
        let s = read(r);
        assert!(s.contains("\"name\":\"Alice\""));
        assert!(s.contains("\"age\":30"));
        assert!(s.contains("\"active\":true"));
    }

    #[test]
    fn test_parse_section() {
        let input = "[server]\nhost = \"localhost\"\nport = 8080";
        let r = ntsc_toml_parse(put(input));
        let s = read(r);
        assert!(s.contains("\"server\""));
        assert!(s.contains("\"host\":\"localhost\""));
        assert!(s.contains("\"port\":8080"));
    }

    #[test]
    fn test_parse_empty() {
        let r = ntsc_toml_parse(put(""));
        assert_eq!(read(r), "{}");
    }

    #[test]
    fn test_parse_comment() {
        let input = "# comment\nname = \"test\"\n# another\nvalue = 42";
        let r = ntsc_toml_parse(put(input));
        let s = read(r);
        assert!(s.contains("\"name\":\"test\""));
        assert!(s.contains("\"value\":42"));
    }

    #[test]
    fn test_stringify() {
        let json = r#"{"name":"Alice","age":30}"#;
        let r = ntsc_toml_stringify(put(json));
        let s = read(r);
        assert!(s.contains("name = \"Alice\""));
        assert!(s.contains("age = 30"));
    }

    #[test]
    fn test_stringify_empty() {
        let r = ntsc_toml_stringify(put("{}"));
        assert_eq!(read(r), "");
    }

    #[test]
    fn test_stringify_not_object() {
        let err = catch_throw(|| {
            let _ = ntsc_toml_stringify(put("[1,2,3]"));
        });
        assert!(err.is_some());
        assert!(err.unwrap().contains("toml.stringify"));
    }

    #[test]
    fn test_parse_inline_table() {
        let input = "server = {host = \"localhost\", port = 8080}";
        let r = ntsc_toml_parse(put(input));
        let s = read(r);
        assert!(s.contains("\"server\""));
        assert!(s.contains("\"host\":\"localhost\""));
        assert!(s.contains("\"port\":8080"));
    }

    #[test]
    fn test_parse_string_with_quotes() {
        let input = r#"msg = "say \\\"hello\\\"""#;
        let r = ntsc_toml_parse(put(input));
        let s = read(r);
        assert!(s.contains("say \\\"hello\\\""), "unexpected output: {s}");
    }
}
