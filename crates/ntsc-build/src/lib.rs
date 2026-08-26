//! Parser for `neutron.toml` — the Neutron project configuration file.
//!
//! Format is TOML with a `[package]` section:
//! ```toml
//! [package]
//! target = "x86_64-unknown-linux-gnu"
//! entry = "src/main.nt"
//! output = "my-project"
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

/// A single error encountered while parsing `neutron.toml`.
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
            "neutron.toml:{}:{}: {}",
            self.line, self.column, self.message
        )
    }
}

impl std::error::Error for BuildError {}

/// Parse a `neutron.toml` source string into a [`BuildConfig`].
///
/// Returns all errors found during parsing (not just the first).
pub fn parse(source: &str) -> Result<BuildConfig, Vec<BuildError>> {
    let mut errors = Vec::new();

    let table: toml::Value = match source.parse() {
        Ok(v) => v,
        Err(e) => {
            errors.push(BuildError {
                message: format!("TOML syntax error: {e}"),
                line: 0,
                column: 0,
            });
            return Err(errors);
        }
    };

    let package = match table.get("package") {
        Some(toml::Value::Table(t)) => t,
        _ => {
            errors.push(BuildError {
                message: "missing `[package]` section".to_string(),
                line: 0,
                column: 0,
            });
            return Err(errors);
        }
    };

    let target = extract_string(package, "target", &mut errors);
    let entry = extract_string(package, "entry", &mut errors);
    let output = extract_string(package, "output", &mut errors);

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

fn extract_string(
    table: &toml::map::Map<String, toml::Value>,
    key: &str,
    errors: &mut Vec<BuildError>,
) -> Option<String> {
    match table.get(key) {
        Some(toml::Value::String(s)) => Some(s.clone()),
        Some(_) => {
            errors.push(BuildError {
                message: format!("`{key}` must be a string"),
                line: 0,
                column: 0,
            });
            None
        }
        None => {
            errors.push(BuildError {
                message: format!("missing required key `{key}` in [package]"),
                line: 0,
                column: 0,
            });
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_config() {
        let src = r#"
[package]
target = "x86_64-unknown-linux-gnu"
entry = "src/main.nt"
output = "my-project"
"#;
        let config = parse(src).expect("should parse successfully");
        assert_eq!(config.target, "x86_64-unknown-linux-gnu");
        assert_eq!(config.entry, "src/main.nt");
        assert_eq!(config.output, "my-project");
    }

    #[test]
    fn valid_with_comments() {
        let src = r#"
# Build configuration
[package]
target = "aarch64-apple-darwin"
entry = "src/main.nt"
output = "cool-app"
"#;
        let config = parse(src).expect("should parse successfully");
        assert_eq!(config.target, "aarch64-apple-darwin");
        assert_eq!(config.entry, "src/main.nt");
        assert_eq!(config.output, "cool-app");
    }

    #[test]
    fn missing_package_section() {
        let src = "target = \"x86_64-unknown-linux-gnu\"\n";
        let errs = parse(src).unwrap_err();
        assert!(
            errs.iter()
                .any(|e| e.message.contains("missing `[package]`"))
        );
    }

    #[test]
    fn missing_target() {
        let src = "[package]\nentry = \"src/main.nt\"\noutput = \"my-project\"\n";
        let errs = parse(src).unwrap_err();
        assert!(
            errs.iter()
                .any(|e| e.message.contains("missing required key `target`"))
        );
    }

    #[test]
    fn missing_entry() {
        let src = "[package]\ntarget = \"x86_64-unknown-linux-gnu\"\noutput = \"my-project\"\n";
        let errs = parse(src).unwrap_err();
        assert!(
            errs.iter()
                .any(|e| e.message.contains("missing required key `entry`"))
        );
    }

    #[test]
    fn missing_output() {
        let src = "[package]\ntarget = \"x86_64-unknown-linux-gnu\"\nentry = \"src/main.nt\"\n";
        let errs = parse(src).unwrap_err();
        assert!(
            errs.iter()
                .any(|e| e.message.contains("missing required key `output`"))
        );
    }

    #[test]
    fn all_missing() {
        let src = "[package]\n";
        let errs = parse(src).unwrap_err();
        assert_eq!(errs.len(), 3);
    }

    #[test]
    fn invalid_toml() {
        let src = "not valid toml {{{";
        let errs = parse(src).unwrap_err();
        assert!(errs.iter().any(|e| e.message.contains("TOML syntax error")));
    }

    #[test]
    fn wrong_type() {
        let src = "[package]\ntarget = 123\nentry = \"src/main.nt\"\noutput = \"p\"\n";
        let errs = parse(src).unwrap_err();
        assert!(errs.iter().any(|e| e.message.contains("must be a string")));
    }
}
