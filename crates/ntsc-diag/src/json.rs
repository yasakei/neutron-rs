//! Machine-readable JSON serialization of diagnostics (for `--json` / IDE use).
//!
//! Deliberately dependency-free: the schema is small and fixed, so hand-rolling
//! the serializer keeps the CLI free of a serde dependency.

use crate::{Diagnostic, Label};

fn diagnostic_to_json(diag: &Diagnostic) -> String {
    let primary = diag
        .labels
        .iter()
        .find(|l| l.is_primary)
        .or_else(|| diag.labels.first());

    let labels = diag
        .labels
        .iter()
        .map(label_to_json)
        .collect::<Vec<_>>()
        .join(",");

    let notes = diag
        .notes
        .iter()
        .map(|n| quote(n))
        .collect::<Vec<_>>()
        .join(",");

    let mut fields = Vec::new();
    fields.push(format!("\"severity\":\"{}\"", diag.severity.as_str()));
    if let Some(code) = &diag.code {
        fields.push(format!("\"code\":{}", quote(code)));
    }
    fields.push(format!("\"message\":{}", quote(&diag.message)));
    if let Some(label) = primary {
        if let Some(file) = &diag.file_path {
            fields.push(format!("\"file\":{}", quote(file)));
        }
        fields.push(format!("\"line\":{}", label.span.line));
        fields.push(format!("\"column\":{}", label.span.column));
        fields.push(format!("\"end_line\":{}", label.span.line));
        fields.push(format!(
            "\"end_column\":{}",
            label.span.column + (label.span.end - label.span.start) as u32
        ));
    }
    if !diag.labels.is_empty() {
        fields.push(format!("\"labels\":[{labels}]"));
    }
    if !diag.notes.is_empty() {
        fields.push(format!("\"notes\":[{notes}]"));
    }
    if let Some(help) = &diag.help {
        fields.push(format!("\"help\":{}", quote(help)));
    }

    format!("{{{}}}", fields.join(","))
}

fn label_to_json(label: &Label) -> String {
    format!(
        "{{\"line\":{},\"column\":{},\"message\":{},\"is_primary\":{}}}",
        label.span.line,
        label.span.column,
        quote(&label.message),
        label.is_primary
    )
}

/// Serialize a batch of diagnostics as one JSON document.
pub fn diagnostics_to_json(diagnostics: &[Diagnostic]) -> String {
    let items = diagnostics
        .iter()
        .map(diagnostic_to_json)
        .collect::<Vec<_>>()
        .join(",");
    format!(
        "{{\"version\":1,\"error_count\":{},\"diagnostics\":[{items}]}}",
        diagnostics
            .iter()
            .filter(|d| d.severity == crate::Severity::Error)
            .count()
    )
}

/// Escape and quote a string for inclusion in JSON output.
fn quote(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Label;
    use ntsc_ast::span::Span;

    #[test]
    fn serializes_basic_diagnostic() {
        let diag = Diagnostic::error("type mismatch")
            .with_code("NTSC-E0201")
            .with_file("src/main.nt")
            .with_label(Label::primary(Span::new(0, 5, 1, 1), "expected int"));
        let doc = diagnostics_to_json(&[diag]);
        assert!(doc.contains("\"severity\":\"error\""));
        assert!(doc.contains("\"code\":\"NTSC-E0201\""));
        assert!(doc.contains("\"message\":\"type mismatch\""));
        assert!(doc.contains("\"file\":\"src/main.nt\""));
        assert!(doc.contains("\"line\":1"));
        assert!(doc.contains("\"error_count\":1"));
    }

    #[test]
    fn escapes_quotes_and_newlines() {
        let diag = Diagnostic::error("say \"hi\"\nbye");
        let doc = diagnostics_to_json(&[diag]);
        assert!(doc.contains("\\\"hi\\\""));
        assert!(doc.contains("\\n"));
    }

    #[test]
    fn counts_only_errors() {
        let err = Diagnostic::error("boom");
        let warn = Diagnostic::warning("warn");
        let doc = diagnostics_to_json(&[err, warn]);
        assert!(doc.contains("\"error_count\":1"));
    }

    #[test]
    fn no_span_yields_minimal_object() {
        let doc = diagnostics_to_json(&[Diagnostic::error("plain")]);
        assert!(!doc.contains("\"line\":"));
    }
}
