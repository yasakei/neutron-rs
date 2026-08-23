//! Pattern nodes for `case` arms in `match` expressions: literals, wildcards,
//! variable bindings, and array/object destructuring.

use crate::expr::LiteralValue;
use crate::span::Span;
use crate::token::Token;


#[derive(Debug, Clone, PartialEq)]
pub enum Pattern {

    Literal { value: LiteralValue, span: Span },

    Wildcard { span: Span },

    Variable { name: Token },

    Array {
        elements: Vec<Pattern>,
        rest: Option<Box<Pattern>>,
        span: Span,
    },

    Object {
        fields: Vec<ObjectPatternField>,
        span: Span,
    },
}


#[derive(Debug, Clone, PartialEq)]
pub struct ObjectPatternField {
    pub key: String,
    pub alias: Option<Token>,
    pub key_span: Span,
}

impl Pattern {

    pub fn span(&self) -> Span {
        match self {
            Pattern::Literal { span, .. } => *span,
            Pattern::Wildcard { span } => *span,
            Pattern::Variable { name } => name.span,
            Pattern::Array { span, .. } => *span,
            Pattern::Object { span, .. } => *span,
        }
    }
}
