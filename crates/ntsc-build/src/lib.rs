//! Parser for `build.ntbl` — the Neutron Build Language configuration file.
//!
//! Format is a simple key-value DSL:
//! ```text
//! target "x86_64-unknown-linux-gnu"
//! entry "src/main.nt"
//! output "my-project"
//! ```

use std::fmt;

pub mod modules;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildConfig {
    /// Target triple for the compiled binary (e.g. `"x86_64-unknown-linux-gnu"`).
    pub target: String,

    /// Path to the entry source file (e.g. `"src/main.nt"`).
    pub entry: String,

    /// Name of the output binary.
    pub output: String,
}

/// A single error encountered while parsing `build.ntbl`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildError {
    pub message: String,
    pub line: u32,
    pub column: u32,
}

impl fmt::Display for BuildError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "build.ntbl:{}:{}: {}",
            self.line, self.column, self.message
        )
    }
}

impl std::error::Error for BuildError {}

/// Parse a `build.ntbl` source string into a [`BuildConfig`].
///
/// Returns all errors found during parsing (not just the first).
pub fn parse(source: &str) -> Result<BuildConfig, Vec<BuildError>> {
    let mut target = None;
    let mut entry = None;
    let mut output = None;
    let mut errors = Vec::new();

    for (line_idx, line) in source.lines().enumerate() {
        let line_num = (line_idx + 1) as u32;
        let trimmed = line.trim();

        // Blank lines and comments are allowed.
        if trimmed.is_empty() || trimmed.starts_with("//") {
            continue;
        }

        let Some((key_part, value_part)) = trimmed.split_once(char::is_whitespace) else {
            errors.push(BuildError {
                message: format!("expected `key \"value\"`, got `{trimmed}`"),
                line: line_num,
                column: 1,
            });
            continue;
        };

        let value_part = value_part.trim();
        if !value_part.starts_with('"') || !value_part.ends_with('"') || value_part.len() < 2 {
            errors.push(BuildError {
                message: format!("expected quoted string, got `{value_part}`"),
                line: line_num,
                column: (key_part.len() + 1) as u32,
            });
            continue;
        }

        // Take everything between the delimiters so quotes inside the value
        // survive.
        let value = &value_part[1..value_part.len() - 1];

        match key_part {
            "target" => {
                if target.is_some() {
                    errors.push(BuildError {
                        message: "`target` specified more than once".to_string(),
                        line: line_num,
                        column: 1,
                    });
                }
                target = Some(value.to_string());
            }
            "entry" => {
                if entry.is_some() {
                    errors.push(BuildError {
                        message: "`entry` specified more than once".to_string(),
                        line: line_num,
                        column: 1,
                    });
                }
                entry = Some(value.to_string());
            }
            "output" => {
                if output.is_some() {
                    errors.push(BuildError {
                        message: "`output` specified more than once".to_string(),
                        line: line_num,
                        column: 1,
                    });
                }
                output = Some(value.to_string());
            }
            other => {
                errors.push(BuildError {
                    message: format!("unknown key `{other}`"),
                    line: line_num,
                    column: 1,
                });
            }
        }
    }

    if target.is_none() {
        errors.push(BuildError {
            message: "missing required key `target`".to_string(),
            line: 0,
            column: 0,
        });
    }
    if entry.is_none() {
        errors.push(BuildError {
            message: "missing required key `entry`".to_string(),
            line: 0,
            column: 0,
        });
    }
    if output.is_none() {
        errors.push(BuildError {
            message: "missing required key `output`".to_string(),
            line: 0,
            column: 0,
        });
    }

    if errors.is_empty() {
        Ok(BuildConfig {
            target: target.expect("checked above"),
            entry: entry.expect("checked above"),
            output: output.expect("checked above"),
        })
    } else {
        Err(errors)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_config() {
        let src =
            "target \"x86_64-unknown-linux-gnu\"\nentry \"src/main.nt\"\noutput \"my-project\"\n";
        let config = parse(src).expect("should parse successfully");
        assert_eq!(config.target, "x86_64-unknown-linux-gnu");
        assert_eq!(config.entry, "src/main.nt");
        assert_eq!(config.output, "my-project");
    }

    #[test]
    fn valid_with_comments_and_blank_lines() {
        let src = "// Build configuration\ntarget \"aarch64-apple-darwin\"\n\nentry \"src/main.nt\"\n// output name\noutput \"cool-app\"\n";
        let config = parse(src).expect("should parse successfully");
        assert_eq!(config.target, "aarch64-apple-darwin");
        assert_eq!(config.entry, "src/main.nt");
        assert_eq!(config.output, "cool-app");
    }

    #[test]
    fn missing_target() {
        let src = "entry \"src/main.nt\"\noutput \"my-project\"\n";
        let errs = parse(src).unwrap_err();
        assert!(
            errs.iter()
                .any(|e| e.message.contains("missing required key `target`"))
        );
    }

    #[test]
    fn missing_entry() {
        let src = "target \"x86_64-unknown-linux-gnu\"\noutput \"my-project\"\n";
        let errs = parse(src).unwrap_err();
        assert!(
            errs.iter()
                .any(|e| e.message.contains("missing required key `entry`"))
        );
    }

    #[test]
    fn missing_output() {
        let src = "target \"x86_64-unknown-linux-gnu\"\nentry \"src/main.nt\"\n";
        let errs = parse(src).unwrap_err();
        assert!(
            errs.iter()
                .any(|e| e.message.contains("missing required key `output`"))
        );
    }

    #[test]
    fn all_missing() {
        let src = "";
        let errs = parse(src).unwrap_err();
        assert_eq!(errs.len(), 3);
    }

    #[test]
    fn unknown_key() {
        let src = "target \"x86_64-unknown-linux-gnu\"\nentry \"src/main.nt\"\noutput \"p\"\nfoo \"bar\"\n";
        let errs = parse(src).unwrap_err();
        assert!(errs.iter().any(|e| e.message.contains("unknown key `foo`")));
    }

    #[test]
    fn duplicate_key() {
        let src = "target \"x86_64-unknown-linux-gnu\"\ntarget \"aarch64-apple-darwin\"\nentry \"src/main.nt\"\noutput \"p\"\n";
        let errs = parse(src).unwrap_err();
        assert!(
            errs.iter()
                .any(|e| e.message.contains("specified more than once"))
        );
    }

    #[test]
    fn unquoted_value() {
        let src = "target x86_64-unknown-linux-gnu\nentry \"src/main.nt\"\noutput \"p\"\n";
        let errs = parse(src).unwrap_err();
        assert!(
            errs.iter()
                .any(|e| e.message.contains("expected quoted string"))
        );
    }

    #[test]
    fn garbage_line() {
        let src = "not-a-valid-line\ntarget \"x86_64-unknown-linux-gnu\"\nentry \"src/main.nt\"\noutput \"p\"\n";
        let errs = parse(src).unwrap_err();
        assert!(
            errs.iter()
                .any(|e| e.message.contains("expected `key \"value\"`"))
        );
    }

    /// Garbage line, unknown key, plus the three missing required keys:
    /// at least four errors.
    #[test]
    fn multiple_errors_collected() {
        let src = "not-a-valid-line\nfoo \"bar\"\nmissing everything\n";
        let errs = parse(src).unwrap_err();

        assert!(errs.len() >= 4);
    }

    #[test]
    fn line_numbers_are_correct() {
        let src = "target \"x86_64-unknown-linux-gnu\"\nentry \"src/main.nt\"\nnotavalidline\noutput \"p\"\n";
        let errs = parse(src).unwrap_err();
        let garbage_err = errs
            .iter()
            .find(|e| e.message.contains("expected `key \"value\"`"))
            .unwrap();
        assert_eq!(garbage_err.line, 3);
    }
}
