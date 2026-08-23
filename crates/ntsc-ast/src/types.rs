use crate::span::Span;
use crate::token::Token;

#[derive(Debug, Clone, PartialEq)]
pub enum TypeAnnotation {
    Int,

    Float,

    String,

    Bool,

    Array(Option<Box<TypeAnnotation>>),

    Object,

    /// `option[Type]` — an explicitly nullable value.
    Option(Box<TypeAnnotation>),

    /// `view T` / `view mut T` — borrows a value for the call instead of
    /// owning it; the bool is the mutability of the borrow.
    View(Box<TypeAnnotation>, bool),

    /// `shared T` — a refcounted reference; assignment and argument passing
    /// retain the value instead of moving it.
    Shared(Box<TypeAnnotation>),

    Any,

    /// `pointer` — an opaque, bounds-checked memory capability.
    Pointer,

    /// `slice[T]` — a bounds-checked window over an array.
    Slice(Option<Box<TypeAnnotation>>),

    /// `own T` — explicit owning allocation.
    Own(Box<TypeAnnotation>),

    /// `&T` / `&mut T` — checked borrows.
    Ref(Box<TypeAnnotation>, bool),

    /// `*const T` / `*mut T` — raw pointers, usable only in unsafe code.
    RawPointer(Box<TypeAnnotation>, bool),

    /// A user-defined named type (e.g. a class name).
    Named(Token),

    /// `impl Trait` in return position, resolved to the concrete class by
    /// the type checker.
    ImplTrait(Token),

    /// `dyn Trait` — a fat pointer pairing an instance with its impl's
    /// vtable.
    Dyn(Token),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewMutability {
    ReadOnly,
    Mutable,
}

impl TypeAnnotation {
    pub fn from_token(token: &Token) -> Option<Self> {
        use crate::token::TokenKind;
        let ann = match &token.kind {
            TokenKind::TypeInt => Self::Int,
            TokenKind::TypeFloat => Self::Float,
            TokenKind::TypeString => Self::String,
            TokenKind::TypeBool => Self::Bool,
            TokenKind::TypeArray => Self::Array(None),
            TokenKind::TypeObject => Self::Object,
            TokenKind::TypeOption => return None,
            TokenKind::TypeAny => Self::Any,
            TokenKind::TypePointer => Self::Pointer,
            TokenKind::TypeSlice => Self::Slice(None),
            TokenKind::Identifier(_) => Self::Named(token.clone()),
            _ => return None,
        };
        Some(ann)
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::Int => "int",
            Self::Float => "float",
            Self::String => "string",
            Self::Bool => "bool",
            Self::Array(_) => "array",
            Self::Object => "object",
            Self::Option(_) => "option",
            Self::View(..) => "view",
            Self::Shared(_) => "shared",
            Self::Any => "any",
            Self::Pointer => "pointer",
            Self::Slice(_) => "slice",
            Self::Own(_) => "own",
            Self::Ref(_, mutable) => {
                if *mutable {
                    "&mut"
                } else {
                    "&"
                }
            }
            Self::RawPointer(_, mutable) => {
                if *mutable {
                    "*mut"
                } else {
                    "*const"
                }
            }
            Self::Named(_) => "named type",
            Self::ImplTrait(_) => "impl trait",
            Self::Dyn(_) => "dyn trait",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ReturnType {
    pub ty: TypeAnnotation,
    pub arrow_span: Span,
}
