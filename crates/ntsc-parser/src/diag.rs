//! Conversion of parser errors into compiler diagnostics.

use ntsc_diag::codes;
use ntsc_diag::{Diagnostic, Label};

use crate::ParseError;

impl From<&ParseError> for Diagnostic {
    fn from(error: &ParseError) -> Self {
        Diagnostic::error(&error.message)
            .with_code(codes::PARSE)
            .with_label(Label::primary(error.span, error.message.clone()))
    }
}

impl From<ParseError> for Diagnostic {
    fn from(error: ParseError) -> Self {
        Diagnostic::from(&error)
    }
}
