//! Span-accurate compiler diagnostics in `rustc` style: severity, error codes,
//! labelled source snippets, notes, and help.

mod json;
mod source;
mod writer;

pub use json::diagnostics_to_json;
pub use source::{SourceBuffer, SourceMap};
pub use writer::{DiagConfig, EmitMode, Writer};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
    Note,
    Help,
}

impl Severity {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Error => "error",
            Self::Warning => "warning",
            Self::Note => "note",
            Self::Help => "help",
        }
    }

    pub fn as_ansi_color(&self) -> &'static str {
        match self {
            Self::Error => "\x1b[1;31m",
            Self::Warning => "\x1b[1;33m",
            Self::Note => "\x1b[0;34m",
            Self::Help => "\x1b[0;32m",
        }
    }
}

pub mod codes {
    // Scheme: `NTSC-<class><nnnn>`, with class `E` for errors and `W` for
    // warnings; each const covers one stage of the pipeline.

    pub const PARSE: &str = "NTSC-E0001";

    pub const RESOLVE: &str = "NTSC-E0101";

    pub const TYPE: &str = "NTSC-E0201";

    pub const CODEGEN: &str = "NTSC-E0301";

    pub const BUILD: &str = "NTSC-E0401";

    pub const OWNERSHIP: &str = "NTSC-E0501";

    pub const WARNING: &str = "NTSC-W0001";
}

#[derive(Debug, Clone)]
pub struct Label {
    pub span: ntsc_ast::span::Span,
    pub message: String,

    /// Whether this is the main label (primary) or a secondary annotation.
    pub is_primary: bool,
}

impl Label {
    pub fn primary(span: ntsc_ast::span::Span, message: impl Into<String>) -> Self {
        Self {
            span,
            message: message.into(),
            is_primary: true,
        }
    }

    #[allow(dead_code)]
    pub fn secondary(span: ntsc_ast::span::Span, message: impl Into<String>) -> Self {
        Self {
            span,
            message: message.into(),
            is_primary: false,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Diagnostic {
    pub severity: Severity,
    pub message: String,
    pub code: Option<String>,
    pub labels: Vec<Label>,
    pub notes: Vec<String>,
    pub help: Option<String>,

    /// Source file path, shown in the `-->` line (absent when there is no
    /// source location).
    pub file_path: Option<String>,

    /// For warnings: the `quiet [name]` list entry that suppresses this
    /// warning locally.
    pub lint: Option<String>,
}

impl Diagnostic {
    pub fn error(message: impl Into<String>) -> Self {
        Self {
            severity: Severity::Error,
            message: message.into(),
            code: None,
            labels: Vec::new(),
            notes: Vec::new(),
            help: None,
            file_path: None,
            lint: None,
        }
    }

    pub fn warning(message: impl Into<String>) -> Self {
        Self {
            severity: Severity::Warning,
            message: message.into(),
            code: None,
            labels: Vec::new(),
            notes: Vec::new(),
            help: None,
            file_path: None,
            lint: None,
        }
    }

    pub fn with_code(mut self, code: impl Into<String>) -> Self {
        self.code = Some(code.into());
        self
    }

    pub fn with_label(mut self, label: Label) -> Self {
        self.labels.push(label);
        self
    }

    pub fn with_labels(mut self, labels: impl IntoIterator<Item = Label>) -> Self {
        self.labels.extend(labels);
        self
    }

    pub fn with_note(mut self, note: impl Into<String>) -> Self {
        self.notes.push(note.into());
        self
    }

    #[allow(dead_code)]
    pub fn with_notes(mut self, notes: impl IntoIterator<Item = String>) -> Self {
        self.notes.extend(notes);
        self
    }

    pub fn with_help(mut self, help: impl Into<String>) -> Self {
        self.help = Some(help.into());
        self
    }

    pub fn with_file(mut self, path: impl Into<String>) -> Self {
        self.file_path = Some(path.into());
        self
    }

    /// Attach the suppressible lint name to a warning (rendered by the JSON
    /// emitter; the text renderer points at `quiet [name]` instead).
    pub fn with_lint(mut self, lint: impl Into<String>) -> Self {
        self.lint = Some(lint.into());
        self
    }

    #[allow(dead_code)]
    /// Convenience constructor for an undefined-symbol error.
    pub fn undefined_symbol(name: &str, span: ntsc_ast::span::Span) -> Self {
        Self::error(format!("undefined variable `{name}`"))
            .with_code("E0401")
            .with_label(Label::primary(
                span,
                format!("`{name}` is not defined here"),
            ))
            .with_note("make sure the variable is declared before use")
    }
}

impl std::fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{}] {}", self.severity.as_str(), self.message)
    }
}
