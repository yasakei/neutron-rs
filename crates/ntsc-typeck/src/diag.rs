//! Conversion of type-checker errors into compiler diagnostics.

use ntsc_diag::codes;
use ntsc_diag::{Diagnostic, Label};

use crate::names::ResolveError;
use crate::resolve::TypeError;
use crate::warnings::Warning;

impl From<&Warning> for Diagnostic {
    fn from(warning: &Warning) -> Self {
        Diagnostic::warning(&warning.message)
            .with_code(codes::WARNING)
            .with_lint(warning.lint)
            .with_label(Label::primary(warning.span, warning.message.clone()))
            .with_help(format!(
                "silence locally with `quiet [{lint}] {{ ... }}`",
                lint = warning.lint
            ))
    }
}

impl From<Warning> for Diagnostic {
    fn from(warning: Warning) -> Self {
        Diagnostic::from(&warning)
    }
}

impl From<&TypeError> for Diagnostic {
    fn from(error: &TypeError) -> Self {
        let mut diag = Diagnostic::error(&error.message)
            .with_code(error.code.unwrap_or(codes::TYPE))
            .with_label(Label::primary(error.span, error.message.clone()));
        if let Some(help) = &error.help {
            diag = diag.with_help(help.clone());
        }
        diag
    }
}

impl From<TypeError> for Diagnostic {
    fn from(error: TypeError) -> Self {
        Diagnostic::from(&error)
    }
}

impl From<&ResolveError> for Diagnostic {
    fn from(error: &ResolveError) -> Self {
        let mut diag = Diagnostic::error(&error.message)
            .with_code(codes::RESOLVE)
            .with_label(Label::primary(error.span, error.message.clone()));
        if let Some(suggestion) = &error.suggestion {
            diag = diag.with_help(format!("did you mean `{suggestion}`?"));
        }
        diag
    }
}

impl From<ResolveError> for Diagnostic {
    fn from(error: ResolveError) -> Self {
        Diagnostic::from(&error)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ntsc_ast::span::Span;

    #[test]
    fn type_error_converts_to_diagnostic() {
        let error = TypeError {
            code: None,
            message: "type mismatch".to_string(),
            span: Span::new(1, 5, 1, 2),
            help: Some("use `copy(...)` to pass an owned value".to_string()),
        };
        let diag = Diagnostic::from(error);
        assert_eq!(diag.code.as_deref(), Some(codes::TYPE));
        assert_eq!(diag.labels[0].span.start, 1);
        assert!(diag.labels[0].is_primary);
        assert!(diag.help.as_deref().unwrap().contains("copy(...)"));
    }

    #[test]
    fn resolve_error_converts_to_diagnostic() {
        let error = ResolveError {
            message: "undefined variable `foo`".to_string(),
            span: Span::new(3, 6, 1, 4),
            suggestion: Some("food".to_string()),
        };
        let diag = Diagnostic::from(error);
        assert_eq!(diag.code.as_deref(), Some(codes::RESOLVE));
        assert_eq!(diag.labels[0].span.start, 3);
        assert_eq!(diag.help.as_deref(), Some("did you mean `food`?"));
    }

    #[test]
    fn resolve_error_without_suggestion_has_no_help() {
        let error = ResolveError {
            message: "undefined variable `foo`".to_string(),
            span: Span::new(3, 6, 1, 4),
            suggestion: None,
        };
        let diag = Diagnostic::from(error);
        assert_eq!(diag.help, None);
    }

    #[test]
    fn warning_converts_to_diagnostic() {
        let warning = crate::warnings::Warning {
            lint: crate::warnings::LINT_UNUSED_VARIABLE,
            message: "unused variable `x`".to_string(),
            span: Span::new(1, 5, 1, 2),
        };
        let diag = Diagnostic::from(warning);
        assert_eq!(diag.severity, ntsc_diag::Severity::Warning);
        assert_eq!(diag.code.as_deref(), Some(codes::WARNING));
        assert_eq!(diag.labels[0].span.start, 1);
        assert_eq!(
            diag.help.as_deref(),
            Some("silence locally with `quiet [unused_variable] { ... }`")
        );
    }
}
