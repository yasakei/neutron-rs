//! NTSC standard library: `glob` module.
//! Glob pattern matching for file path selection and ignore rules.

use crate::registry;

fn fail(fn_name: &str, msg: impl std::fmt::Display) -> i64 {
    super::throw_str(format!("glob.{fn_name}: {msg}"))
}

/// Simple glob-to-regex converter. Supports `*`, `**`, `?`, and `[...]`
/// character classes. Does not support brace expansion `{a,b}`.
fn glob_to_regex(pattern: &str) -> Result<String, String> {
    let mut regex = String::from("^");
    let mut chars = pattern.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '*' => {
                if chars.peek() == Some(&'*') {
                    chars.next();
                    // `**` matches everything including path separators.
                    regex.push_str(".*");
                } else {
                    // `*` matches everything except path separators.
                    regex.push_str("[^/]*");
                }
            }
            '?' => regex.push_str("[^/]"),
            '[' => {
                let mut class = String::from("[");
                // Support negation `[^...]`
                if chars.peek() == Some(&'^') || chars.peek() == Some(&'!') {
                    class.push('^');
                    chars.next();
                }
                let mut prev = '\0';
                loop {
                    match chars.next() {
                        Some(']') if prev != '\0' || class.len() > 1 => {
                            class.push(']');
                            break;
                        }
                        Some(c) => {
                            // Escape regex-special characters inside the class.
                            match c {
                                '\\' => {
                                    if let Some(escaped) = chars.next() {
                                        class.push('\\');
                                        class.push(escaped);
                                        prev = escaped;
                                    }
                                }
                                '-' if !class.ends_with('[') && prev != '\0' => {
                                    class.push('-');
                                    prev = '\0';
                                }
                                '.' | '^' | '$' | '+' | '(' | ')' | '{' | '}' | '|' => {
                                    class.push('\\');
                                    class.push(c);
                                    prev = c;
                                }
                                _ => {
                                    class.push(c);
                                    prev = c;
                                }
                            }
                        }
                        None => {
                            // Unterminated class — treat `[` as literal.
                            regex.push_str("\\[");
                            regex.push_str(&class[1..]);
                            break;
                        }
                    }
                }
                regex.push_str(&class);
            }
            '\\' => {
                if let Some(escaped) = chars.next() {
                    regex.push('\\');
                    regex.push(escaped);
                }
            }
            '.' | '^' | '$' | '+' | '(' | ')' | '{' | '}' | '|' => {
                regex.push('\\');
                regex.push(c);
            }
            _ => regex.push(c),
        }
    }
    regex.push('$');
    Ok(regex)
}

/// `glob.matches(pattern, path)` — returns true if `path` matches the glob
/// `pattern`.
#[unsafe(no_mangle)]
pub extern "C" fn ntsc_glob_matches(pattern: i64, path: i64) -> i8 {
    let pattern = registry::get_string(pattern).unwrap_or_default();
    let path = registry::get_string(path).unwrap_or_default();
    match glob_to_regex(&pattern) {
        Ok(regex_str) => match regex::Regex::new(&regex_str) {
            Ok(re) => {
                if re.is_match(&path) {
                    1
                } else {
                    0
                }
            }
            Err(_) => 0,
        },
        Err(_) => 0,
    }
}

/// `glob.find(root, pattern)` — returns a newline-separated list of paths
/// under `root` that match the glob `pattern`. Only regular files are
/// returned.
#[unsafe(no_mangle)]
pub extern "C" fn ntsc_glob_find(root: i64, pattern: i64) -> i64 {
    let root_str = registry::get_string(root).unwrap_or_default();
    let pattern_str = registry::get_string(pattern).unwrap_or_default();

    let re_str = match glob_to_regex(&pattern_str) {
        Ok(r) => r,
        Err(e) => return fail("find", e),
    };
    let re = match regex::Regex::new(&re_str) {
        Ok(r) => r,
        Err(e) => return fail("find", format!("invalid pattern: {e}")),
    };

    let root_path = std::path::Path::new(&root_str);
    if !root_path.is_dir() {
        return fail("find", format!("'{root_str}' is not a directory"));
    }

    let mut results = Vec::new();
    let walker = WalkDir {
        stack: vec![root_path.to_path_buf()],
    };
    for entry in walker {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };
        if !entry.is_file() {
            continue;
        }
        let relative = entry.strip_prefix(&root_str).unwrap_or(&entry);
        let rel_str = relative.to_string_lossy().to_string();
        if re.is_match(&rel_str) || re.is_match(&entry.to_string_lossy()) {
            results.push(rel_str);
        }
    }

    registry::put_string(results.join("\n"))
}

/// `glob.is_match(pattern, path)` — alias for `matches`, returns 0 or 1.
#[unsafe(no_mangle)]
pub extern "C" fn ntsc_glob_is_match(pattern: i64, path: i64) -> i8 {
    ntsc_glob_matches(pattern, path)
}

/// Simple directory walker — no external dependencies needed.
struct WalkDir {
    stack: Vec<std::path::PathBuf>,
}

impl Iterator for WalkDir {
    type Item = Result<std::path::PathBuf, std::io::Error>;

    fn next(&mut self) -> Option<Self::Item> {
        let path = self.stack.pop()?;
        if path.is_dir() {
            match std::fs::read_dir(&path) {
                Ok(entries) => {
                    let mut dirs: Vec<std::path::PathBuf> = Vec::new();
                    for entry in entries.flatten() {
                        let entry_path = entry.path();
                        if entry_path.is_dir() {
                            dirs.push(entry_path);
                        } else {
                            self.stack.push(entry_path);
                        }
                    }
                    // Push directories in reverse so they are processed
                    // in alphabetical order.
                    dirs.sort();
                    dirs.reverse();
                    self.stack.extend(dirs);
                }
                Err(e) => return Some(Err(e)),
            }
        }
        Some(Ok(path))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn put(s: &str) -> i64 {
        registry::put_string(s.to_string())
    }

    #[test]
    fn test_glob_to_regex_simple() {
        assert!(glob_to_regex("*.txt").is_ok());
    }

    #[test]
    fn test_matches_simple() {
        assert_eq!(ntsc_glob_matches(put("*.txt"), put("hello.txt")), 1);
        assert_eq!(ntsc_glob_matches(put("*.txt"), put("hello.rs")), 0);
    }

    #[test]
    fn test_matches_question_mark() {
        assert_eq!(ntsc_glob_matches(put("a?.txt"), put("ab.txt")), 1);
        assert_eq!(ntsc_glob_matches(put("a?.txt"), put("a.txt")), 0);
    }

    #[test]
    fn test_matches_double_star() {
        assert_eq!(ntsc_glob_matches(put("**/*.txt"), put("a/b/c.txt")), 1);
        assert_eq!(ntsc_glob_matches(put("**/*.txt"), put("a/b/c.rs")), 0);
    }

    #[test]
    fn test_matches_char_class() {
        assert_eq!(ntsc_glob_matches(put("[abc].txt"), put("a.txt")), 1);
        assert_eq!(ntsc_glob_matches(put("[abc].txt"), put("d.txt")), 0);
    }
}
