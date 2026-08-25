//! NTSC standard library: `yaml` module.
//! YAML values are compact YAML strings; arguments are borrowed handles,
//! returned handles are owned by the caller.

use crate::registry;

use crate::modules::unescape;

fn count_indent(line: &str) -> usize {
    line.bytes()
        .take_while(|&b| b == b' ' || b == b'\t')
        .count()
}

fn parse_yaml_value(val: &str) -> String {
    let val = val.trim();
    if val.is_empty() {
        return "\"\"".to_string();
    }
    if (val.starts_with('"') && val.ends_with('"') && val.len() >= 2)
        || (val.starts_with('\'') && val.ends_with('\'') && val.len() >= 2)
    {
        let inner = &val[1..val.len() - 1];
        return format!(
            "\"{}\"",
            inner
                .replace('\\', "\\\\")
                .replace('"', "\\\"")
                .replace('\n', "\\n")
        );
    }
    if val == "true" {
        return "true".to_string();
    }
    if val == "false" {
        return "false".to_string();
    }
    if val == "null" || val == "~" {
        return "null".to_string();
    }
    if let Ok(n) = val.parse::<i64>() {
        return n.to_string();
    }
    if let Ok(f) = val.parse::<f64>() {
        return format!("{f}");
    }
    if val.starts_with('[') && val.ends_with(']') {
        let inner = &val[1..val.len() - 1].trim();
        if inner.is_empty() {
            return "[]".to_string();
        }
        let items: Vec<String> = split_flow_sequence(inner)
            .iter()
            .map(|s| parse_yaml_value(s))
            .collect();
        return format!("[{}]", items.join(","));
    }
    if val.starts_with('{') && val.ends_with('}') {
        let inner = &val[1..val.len() - 1].trim();
        if inner.is_empty() {
            return "{}".to_string();
        }
        let pairs: Vec<String> = split_flow_mapping(inner)
            .iter()
            .map(|s| {
                if let Some(colon) = s.find(':') {
                    let k = s[..colon].trim();
                    let v = s[colon + 1..].trim();
                    format!("\"{}\":{}", k, parse_yaml_value(v))
                } else {
                    format!("\"{}\":null", s.trim())
                }
            })
            .collect();
        return format!("{{{}}}", pairs.join(","));
    }
    let escaped = val
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n");
    format!("\"{escaped}\"")
}

fn split_flow_sequence(s: &str) -> Vec<String> {
    let mut items = Vec::new();
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
        if c == '"' || c == '\'' {
            in_str = !in_str;
            current.push(c);
            continue;
        }
        if in_str {
            current.push(c);
            continue;
        }
        if c == '[' || c == '{' {
            depth += 1;
            current.push(c);
        } else if c == ']' || c == '}' {
            depth -= 1;
            current.push(c);
        } else if c == ',' && depth == 0 {
            let trimmed = current.trim().to_string();
            if !trimmed.is_empty() {
                items.push(trimmed);
            }
            current.clear();
        } else {
            current.push(c);
        }
    }
    let trimmed = current.trim().to_string();
    if !trimmed.is_empty() {
        items.push(trimmed);
    }
    items
}

fn split_flow_mapping(s: &str) -> Vec<String> {
    split_flow_sequence(s)
}

#[unsafe(no_mangle)]
pub extern "C" fn ntsc_yaml_parse(s: i64) -> i64 {
    let input = registry::get_string(s).unwrap_or_default();
    let input = unescape(&input);
    let lines: Vec<&str> = input.lines().collect();
    if lines.is_empty() {
        return registry::put_string("{}".to_string());
    }
    let mut result = String::from("{");
    let mut first = true;
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            i += 1;
            continue;
        }
        if trimmed.starts_with("- ") || trimmed == "-" {
            return super::throw_str(
                "yaml.parse: top-level sequences are not supported, use inline flow".to_string(),
            );
        }
        if let Some(colon_pos) = trimmed.find(':') {
            let key = trimmed[..colon_pos].trim();
            let val_part = trimmed[colon_pos + 1..].trim();
            if val_part.is_empty() {
                let base_indent = count_indent(line);
                let mut nested_lines = Vec::new();
                i += 1;
                while i < lines.len() {
                    let next_line = lines[i];
                    let next_trimmed = next_line.trim();
                    if next_trimmed.is_empty() || next_trimmed.starts_with('#') {
                        i += 1;
                        continue;
                    }
                    let next_indent = count_indent(next_line);
                    if next_indent <= base_indent {
                        break;
                    }
                    nested_lines.push(next_line);
                    i += 1;
                }
                if !nested_lines.is_empty() && nested_lines[0].trim().starts_with("- ") {
                    let mut items = Vec::new();
                    for nl in &nested_lines {
                        let nt = nl.trim();
                        if let Some(rest) = nt.strip_prefix("- ") {
                            items.push(parse_yaml_value(rest));
                        }
                    }
                    if !first {
                        result.push(',');
                    }
                    result.push_str(&format!("\"{}\":[{}]", key, items.join(",")));
                    first = false;
                } else if !nested_lines.is_empty() {
                    let mut nested_obj = String::new();
                    let mut nf = true;
                    for nl in &nested_lines {
                        let nt = nl.trim();
                        if let Some(nc) = nt.find(':') {
                            let nk = nt[..nc].trim();
                            let nv = nt[nc + 1..].trim();
                            if !nf {
                                nested_obj.push(',');
                            }
                            if nv.is_empty() {
                                nested_obj.push_str(&format!("\"{nk}\":{{}}"));
                            } else {
                                nested_obj.push_str(&format!("\"{nk}\":{}", parse_yaml_value(nv)));
                            }
                            nf = false;
                        }
                    }
                    if !first {
                        result.push(',');
                    }
                    result.push_str(&format!("\"{key}\":{{{nested_obj}}}"));
                    first = false;
                } else {
                    if !first {
                        result.push(',');
                    }
                    result.push_str(&format!("\"{key}\":{{}}"));
                    first = false;
                }
                continue;
            } else {
                let val = parse_yaml_value(val_part);
                if !first {
                    result.push(',');
                }
                result.push_str(&format!("\"{key}\":{val}"));
                first = false;
            }
        }
        i += 1;
    }
    result.push('}');
    registry::put_string(result)
}

#[unsafe(no_mangle)]
pub extern "C" fn ntsc_yaml_stringify(s: i64) -> i64 {
    let json = registry::get_string(s).unwrap_or_default();
    let json = json.trim();
    if json.is_empty() || json == "{}" {
        return registry::put_string(String::new());
    }
    if json.starts_with('[') && json.ends_with(']') {
        return super::throw_str(
            "yaml.stringify: top-level sequences are not supported".to_string(),
        );
    }
    if !json.starts_with('{') {
        return super::throw_str("yaml.stringify: expected a JSON object".to_string());
    }
    let inner = &json[1..json.len() - 1].trim();
    if inner.is_empty() {
        return registry::put_string(String::new());
    }
    let mut result = String::new();
    let entries = match split_flow_sequence(inner) {
        e if !e.is_empty() => e,
        _ => {
            return super::throw_str("yaml.stringify: malformed JSON object".to_string());
        }
    };
    for (idx, entry) in entries.iter().enumerate() {
        if let Some(colon_pos) = entry.find(':') {
            let key = entry[..colon_pos].trim().trim_matches('"');
            let val = entry[colon_pos + 1..].trim();
            if val.starts_with('{') && val.ends_with('}') {
                let nested_inner = &val[1..val.len() - 1].trim();
                if nested_inner.is_empty() {
                    if idx > 0 {
                        result.push('\n');
                    }
                    result.push_str(&format!("{key}:\n"));
                } else {
                    if idx > 0 {
                        result.push('\n');
                    }
                    result.push_str(&format!("{key}:\n"));
                    let nested_entries = split_flow_sequence(nested_inner);
                    for ne in &nested_entries {
                        if let Some(nc) = ne.find(':') {
                            let nk = ne[..nc].trim().trim_matches('"');
                            let nv = ne[nc + 1..].trim();
                            result.push_str(&format!("  {nk}: {}\n", json_to_yaml_value(nv)));
                        }
                    }
                }
            } else if val.starts_with('[') && val.ends_with(']') {
                if idx > 0 {
                    result.push('\n');
                }
                result.push_str(&format!("{key}:\n"));
                let arr_inner = &val[1..val.len() - 1].trim();
                if !arr_inner.is_empty() {
                    let items = split_flow_sequence(arr_inner);
                    for item in &items {
                        result.push_str(&format!("- {}\n", json_to_yaml_value(item)));
                    }
                }
            } else {
                if idx > 0 {
                    result.push('\n');
                }
                result.push_str(&format!("{key}: {}\n", json_to_yaml_value(val)));
            }
        }
    }
    registry::put_string(result)
}

fn json_to_yaml_value(val: &str) -> String {
    if val.starts_with('"') && val.ends_with('"') && val.len() >= 2 {
        let inner = &val[1..val.len() - 1];
        if inner.contains(':')
            || inner.contains('#')
            || inner.starts_with(' ')
            || inner.ends_with(' ')
        {
            format!("\"{inner}\"")
        } else {
            inner.to_string()
        }
    } else if val == "null" {
        "null".to_string()
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
        let input = "name: Alice\nage: 30\nactive: true";
        let r = ntsc_yaml_parse(put(input));
        let s = read(r);
        assert!(s.contains("\"name\":\"Alice\""));
        assert!(s.contains("\"age\":30"));
        assert!(s.contains("\"active\":true"));
    }

    #[test]
    fn test_parse_empty() {
        let r = ntsc_yaml_parse(put(""));
        assert_eq!(read(r), "{}");
    }

    #[test]
    fn test_parse_nested() {
        let input = "server:\n  host: localhost\n  port: 8080";
        let r = ntsc_yaml_parse(put(input));
        let s = read(r);
        assert!(s.contains("\"server\""));
        assert!(s.contains("\"host\":\"localhost\""));
        assert!(s.contains("\"port\":8080"));
    }

    #[test]
    fn test_parse_flow_sequence() {
        let input = "nums: [1, 2, 3]";
        let r = ntsc_yaml_parse(put(input));
        let s = read(r);
        assert!(s.contains("\"nums\":[1,2,3]"));
    }

    #[test]
    fn test_parse_comments() {
        let input = "# comment\nname: test\n# another\nvalue: 42";
        let r = ntsc_yaml_parse(put(input));
        let s = read(r);
        assert!(s.contains("\"name\":\"test\""));
        assert!(s.contains("\"value\":42"));
    }

    #[test]
    fn test_stringify() {
        let json = r#"{"name":"Alice","age":30}"#;
        let r = ntsc_yaml_stringify(put(json));
        let s = read(r);
        assert!(s.contains("name: Alice"));
        assert!(s.contains("age: 30"));
    }

    #[test]
    fn test_stringify_empty() {
        let r = ntsc_yaml_stringify(put("{}"));
        assert_eq!(read(r), "");
    }

    #[test]
    fn test_stringify_nested() {
        let json = r#"{"server":{"host":"localhost","port":8080}}"#;
        let r = ntsc_yaml_stringify(put(json));
        let s = read(r);
        assert!(s.contains("server:"));
        assert!(s.contains("host: localhost"));
        assert!(s.contains("port: 8080"));
    }

    #[test]
    fn test_stringify_array() {
        let json = r#"{"items":[1,2,3]}"#;
        let r = ntsc_yaml_stringify(put(json));
        let s = read(r);
        assert!(s.contains("items:"));
        assert!(s.contains("- 1"));
        assert!(s.contains("- 2"));
        assert!(s.contains("- 3"));
    }

    #[test]
    fn test_parse_flow_mapping() {
        let input = "server: {host: localhost, port: 8080}";
        let r = ntsc_yaml_parse(put(input));
        let s = read(r);
        assert!(s.contains("\"server\""));
        assert!(s.contains("\"host\":\"localhost\""));
        assert!(s.contains("\"port\":8080"));
    }

    #[test]
    fn test_stringify_not_object() {
        let err = catch_throw(|| {
            let _ = ntsc_yaml_stringify(put("[1,2,3]"));
        });
        assert!(err.is_some());
        assert!(err.unwrap().contains("yaml.stringify"));
    }

    #[test]
    fn test_parse_null_values() {
        let input = "name: Alice\nvalue: null";
        let r = ntsc_yaml_parse(put(input));
        let s = read(r);
        assert!(s.contains("\"name\":\"Alice\""));
        assert!(s.contains("\"value\":null"));
    }

    #[test]
    fn test_parse_array_items() {
        let input = "fruits:\n  - apple\n  - banana\n  - cherry";
        let r = ntsc_yaml_parse(put(input));
        let s = read(r);
        assert!(s.contains("\"fruits\""));
        assert!(s.contains("\"apple\""));
        assert!(s.contains("\"banana\""));
        assert!(s.contains("\"cherry\""));
    }
}
