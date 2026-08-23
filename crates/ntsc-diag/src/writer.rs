//! Pretty-printing compiler diagnostics in `rustc` style: header line, source
//! snippet with labelled underlines, notes, and help.

use crate::source::{SourceBuffer, SourceMap};
use crate::{Diagnostic, Severity};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EmitMode {
    Coloured,

    Plain,
}

#[derive(Debug, Clone)]
pub struct DiagConfig {
    pub mode: EmitMode,

    /// Stop rendering after this many error diagnostics; `None` means no limit.
    pub max_errors: Option<usize>,
}

impl DiagConfig {
    pub fn from_env() -> Self {
        Self::from_env_with(
            |name| std::env::var(name).ok(),
            std::io::IsTerminal::is_terminal(&std::io::stderr()),
        )
    }

    fn from_env_with(mut get_var: impl FnMut(&str) -> Option<String>, is_tty: bool) -> Self {
        // Colour is forced on by `CLICOLOR_FORCE` (non-empty, not "0"); `NO_COLOR`,
        // `CLICOLOR=0` and `TERM=dumb` force it off; otherwise colour only on a TTY.
        let force = get_var("CLICOLOR_FORCE")
            .map(|v| !v.is_empty() && v != "0")
            .unwrap_or(false);
        let no_color = get_var("NO_COLOR").map(|v| !v.is_empty()).unwrap_or(false);
        let clicolor_off = get_var("CLICOLOR").map(|v| v == "0").unwrap_or(false);
        let dumb_term = get_var("TERM").map(|v| v == "dumb").unwrap_or(false);

        let colour = force || (!no_color && !clicolor_off && !dumb_term && is_tty);

        // Error limit: defaults to 20; `0` means unlimited.
        let max_errors = match get_var("NTSC_MAX_ERRORS").and_then(|v| v.parse::<usize>().ok()) {
            Some(0) => None,
            Some(n) => Some(n),
            None => Some(20),
        };
        Self {
            mode: if colour {
                EmitMode::Coloured
            } else {
                EmitMode::Plain
            },
            max_errors,
        }
    }
}

impl Default for DiagConfig {
    fn default() -> Self {
        Self {
            mode: EmitMode::Coloured,
            max_errors: None,
        }
    }
}

pub struct Writer {
    config: DiagConfig,
}

impl Writer {
    pub fn new(config: DiagConfig) -> Self {
        Self { config }
    }

    /// Pretty-print one diagnostic, resolving its source snippet from
    /// `sources` by `diag.file_path`.
    pub fn emit(&self, diag: &Diagnostic, sources: Option<&SourceMap>) {
        let mut buf = String::new();
        emit_diag(&mut buf, diag, sources, &self.config);
        eprintln!("{buf}");
    }

    /// Print all diagnostics, stopping once `max_errors` error diagnostics
    /// have been rendered (warnings do not count toward the limit).
    pub fn emit_all(&self, diagnostics: &[Diagnostic], sources: Option<&SourceMap>) {
        let error_count = diagnostics
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .count();
        let limit = self.config.max_errors.unwrap_or(usize::MAX);

        let mut rendered_errors = 0usize;
        for (rendered, diag) in diagnostics.iter().enumerate() {
            if diag.severity == Severity::Error {
                if rendered_errors >= limit {
                    break;
                }
                rendered_errors += 1;
            }
            if rendered > 0 {
                eprintln!();
            }
            self.emit(diag, sources);
        }

        let omitted = error_count.saturating_sub(rendered_errors);
        if omitted > 0 {
            let mode = self.config.mode;
            let color = severity_color(Severity::Error, mode);
            let reset = ansi_reset(mode);
            eprintln!(
                "{color}error{reset}: {omitted} more error{} not shown",
                if omitted == 1 { "" } else { "s" }
            );
        }

        if error_count > 0 {
            let mode = self.config.mode;
            let color = severity_color(Severity::Error, mode);
            let reset = ansi_reset(mode);
            eprintln!(
                "{color}error{reset}: aborting due to {error_count} previous error{}",
                if error_count == 1 { "" } else { "s" }
            );
        }
    }
}

/// Core formatting into a string buffer (kept separate from `Writer` to avoid
/// borrow conflicts).
fn emit_diag(
    buf: &mut String,
    diag: &Diagnostic,
    sources: Option<&SourceMap>,
    config: &DiagConfig,
) {
    use std::fmt::Write;

    let mode = config.mode;
    let color = severity_color(diag.severity, mode);
    let reset = ansi_reset(mode);
    let bold = ansi_bold(mode);

    // ── header line ────────────────────────────────────────────────
    let sev_str = if let Some(ref code) = diag.code {
        format!("{}[{}]", diag.severity.as_str(), code)
    } else {
        diag.severity.as_str().to_string()
    };
    let _ = write!(
        buf,
        "{color}{sev_str}{reset}: {bold}{}{reset}",
        diag.message
    );

    // ── file location ──────────────────────────────────────────────
    let primary_span = diag
        .labels
        .iter()
        .find(|l| l.is_primary)
        .or_else(|| diag.labels.first())
        .map(|l| l.span);

    if let Some(span) = primary_span {
        let file = diag.file_path.as_deref().unwrap_or("<unknown>");
        let short = shorten_path(file);
        let _ = write!(
            buf,
            "\n{color}  -->{reset} {short}:{}:{}",
            span.line, span.column
        );
    }

    // ── source snippet with annotations ────────────────────────────
    let source = primary_span.and_then(|_| {
        diag.file_path
            .as_deref()
            .and_then(|p| sources.and_then(|m| m.get(p)))
    });

    if let (Some(source), Some(span)) = (source, primary_span) {
        let line = span.line as usize;
        if line > 0 && line <= source.line_count() {
            let digits = source.line_count().to_string().len().max(2);
            let prefix = |l: usize| -> String { SourceBuffer::line_number_prefix(l, digits) };

            let text = source.line_text(line);

            let _ = write!(buf, "\n{}  |", bold);
            let _ = write!(buf, "\n{}{}{reset}{}", color, prefix(line), text);

            let line_labels: Vec<_> = diag
                .labels
                .iter()
                .filter(|lb| {
                    source.line_at_offset(lb.span.start) == line
                        || source.line_at_offset(lb.span.end) == line
                })
                .collect();

            if line_labels.is_empty() {
                let col_offset = span.column.saturating_sub(1) as usize;
                let width = (span.end - span.start).max(1);
                let mut uline = String::new();
                for _ in 0..(prefix(line).len() + col_offset) {
                    uline.push(' ');
                }
                for _ in 0..width {
                    uline.push('^');
                }
                let _ = write!(buf, "\n{uline}");
            } else {
                let mut sorted: Vec<_> = line_labels.iter().collect();
                sorted.sort_by_key(|lb| lb.span.start);

                for label in &sorted {
                    let lcol = label.span.column.saturating_sub(1) as usize;
                    let lwidth = (label.span.end - label.span.start).max(1);
                    let mut uline = String::new();
                    for _ in 0..(prefix(line).len() + lcol) {
                        uline.push(' ');
                    }
                    for _ in 0..lwidth {
                        uline.push('^');
                    }
                    if label.is_primary {
                        let _ = write!(buf, "\n{uline} {color}{}{reset}", label.message);
                    } else {
                        let lc = ansi_color("\x1b[0;35m", mode);
                        let _ = write!(buf, "\n{uline} {lc}{}{reset}", label.message);
                    }
                }
            }
            let _ = write!(buf, "\n{}  |", bold);
        }
    }

    // ── notes ──────────────────────────────────────────────────────
    let note_color = ansi_color("\x1b[0;34m", mode);
    for note in &diag.notes {
        let _ = write!(buf, "\n{note_color}   ={reset} note: {note}");
    }

    // ── help ────────────────────────────────────────────────────────
    if let Some(ref help) = diag.help {
        let help_color = ansi_color("\x1b[0;32m", mode);
        let _ = write!(buf, "\n{help_color}   ={reset} help: {help}");
    }
}

fn ansi_color(code: &'static str, mode: EmitMode) -> &'static str {
    match mode {
        EmitMode::Coloured => code,
        EmitMode::Plain => "",
    }
}

fn severity_color(severity: Severity, mode: EmitMode) -> &'static str {
    ansi_color(severity.as_ansi_color(), mode)
}

fn ansi_reset(mode: EmitMode) -> &'static str {
    ansi_color("\x1b[0m", mode)
}

fn ansi_bold(mode: EmitMode) -> &'static str {
    ansi_color("\x1b[1m", mode)
}

/// Shorten a path for display by stripping the current working directory prefix.
fn shorten_path(path: &str) -> String {
    if let Ok(cwd) = std::env::current_dir() {
        let p = std::path::Path::new(path);
        if let Ok(relative) = p.strip_prefix(&cwd) {
            return relative.display().to_string();
        }
    }
    path.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source::SourceBuffer;
    use crate::{Diagnostic, Label};
    use ntsc_ast::span::Span;
    use std::collections::HashMap;

    fn config_from(values: &[(&str, &str)]) -> DiagConfig {
        let values: HashMap<&str, &str> = values.iter().copied().collect();
        DiagConfig::from_env_with(
            |name| values.get(name).map(|value| (*value).to_string()),
            false,
        )
    }
    fn plain_writer() -> Writer {
        Writer::new(DiagConfig {
            mode: EmitMode::Plain,
            max_errors: None,
        })
    }

    fn writer_with_limit(limit: usize) -> Writer {
        Writer::new(DiagConfig {
            mode: EmitMode::Plain,
            max_errors: Some(limit),
        })
    }

    #[test]
    fn header_format() {
        let diag = Diagnostic::error("type mismatch")
            .with_code("E0308")
            .with_file("test.nt");
        plain_writer().emit(&diag, None);
    }

    #[test]
    fn with_source_snippet() {
        let source = SourceBuffer::new("var int x = \"hello\"\n", "test.nt");
        let diag = Diagnostic::error("type mismatch: expected `int`, got `string`")
            .with_code("E0308")
            .with_file("test.nt")
            .with_label(Label::primary(
                Span::new(8, 15, 1, 9),
                "expected `int`, got `string`",
            ))
            .with_note("variables cannot change type after declaration");
        let mut map = SourceMap::new();
        map.add(source);
        plain_writer().emit(&diag, Some(&map));
    }

    #[test]
    fn multiple_labels() {
        let source = SourceBuffer::new("var x = 1 + \"hello\"\n", "test.nt");
        let diag = Diagnostic::error("binary operator `+` cannot apply to `int` and `string`")
            .with_code("E0308")
            .with_file("test.nt")
            .with_label(Label::primary(Span::new(8, 9, 1, 9), "int"))
            .with_label(Label::secondary(Span::new(12, 19, 1, 13), "string"));
        let mut map = SourceMap::new();
        map.add(source);
        plain_writer().emit(&diag, Some(&map));
    }

    #[test]
    fn source_map_ignores_unknown_path() {
        let source = SourceBuffer::new("var x = 1\n", "other.nt");
        let diag = Diagnostic::error("oops")
            .with_file("test.nt")
            .with_label(Label::primary(Span::new(0, 1, 1, 1), "here"));
        let mut map = SourceMap::new();
        map.add(source);

        plain_writer().emit(&diag, Some(&map));
    }

    #[test]
    fn colour_config_respects_no_color_env() {
        let config = config_from(&[("NO_COLOR", "1")]);
        assert_eq!(config.mode, EmitMode::Plain);
    }

    #[test]
    fn colour_config_forces_colour() {
        let config = config_from(&[("CLICOLOR_FORCE", "1"), ("NO_COLOR", "1")]);
        assert_eq!(config.mode, EmitMode::Coloured);
    }

    #[test]
    fn from_env_defaults_error_limit_to_20() {
        assert_eq!(config_from(&[]).max_errors, Some(20));
    }

    #[test]
    fn from_env_reads_error_limit() {
        assert_eq!(config_from(&[("NTSC_MAX_ERRORS", "3")]).max_errors, Some(3));
    }

    #[test]
    fn from_env_zero_means_unlimited() {
        assert_eq!(config_from(&[("NTSC_MAX_ERRORS", "0")]).max_errors, None);
    }

    #[test]
    fn error_limit_caps_rendered_errors() {
        let diags: Vec<Diagnostic> = (0..5)
            .map(|i| {
                Diagnostic::error(format!("error number {i}"))
                    .with_code("E0")
                    .with_label(Label::primary(Span::new(i, i + 1, 1, 1), "here"))
            })
            .collect();
        let writer = writer_with_limit(2);

        writer.emit_all(&diags, None);
        assert_eq!(diags.len(), 5);
    }
}
