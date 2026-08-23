//! Resolved type representation.

use std::fmt;

/// A fully-resolved type in the NTSC type system.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ty {
    /// 64-bit signed integer.
    Int,

    /// 64-bit floating point.
    Float,

    /// UTF-8 string.
    String,

    /// Boolean.
    Bool,

    /// Unit / no return value.
    Void,

    /// The `nil` literal type.
    Nil,

    /// Dynamic array of elements.
    Array(Box<Ty>),

    /// Explicitly nullable value.
    Option(Box<Ty>),

    /// Object / record type.
    Object,

    /// Function type with parameter types and return type.
    Function {
        params: Vec<Ty>,
        return_type: Box<Ty>,
    },

    /// User-defined class type by name.
    Class(String),

    /// A block-scoped, non-owning view of a value. The `bool` is `true`
    /// for an exclusive mutable view (`view mut`). Views only arise from
    /// `view`/`view mut` expressions and view-typed parameters; they
    /// cannot be stored.
    View(Box<Ty>, bool),

    /// An explicitly shared, refcounted reference to a heap value (the
    /// escape hatch for aliasing). Sharing is opt-in: only heap types may
    /// be wrapped. Shared values are never moved — assignments, argument
    /// passing, and returns copy the reference and retain the box.
    Shared(Box<Ty>),

    /// Type-erased — accepts anything (escape hatch).
    Any,

    /// An opaque, bounds-checked memory capability.
    Pointer,

    /// A bounds-checked window over an array.
    Slice(Box<Ty>),
    Own(Box<Ty>),
    Ref(Box<Ty>, bool),
    RawPointer(Box<Ty>, bool),

    /// A trait object (`dyn P`): a fat pointer to an instance of some
    /// class implementing `P` plus that impl's vtable. The value owns both
    /// the header and the wrapped instance.
    Dyn(String),
}

impl Ty {
    /// Human-readable label for diagnostics.
    pub fn label(&self) -> String {
        match self {
            Self::Int => "int".into(),
            Self::Float => "float".into(),
            Self::String => "string".into(),
            Self::Bool => "bool".into(),
            Self::Void => "void".into(),
            Self::Nil => "nil".into(),
            Self::Array(inner) => format!("array<{inner}>"),
            Self::Option(inner) => format!("option<{inner}>"),
            Self::Object => "object".into(),
            Self::Function {
                params,
                return_type,
            } => {
                let params_str: Vec<_> = params.iter().map(|p| p.label()).collect();
                format!("fun({}) -> {return_type}", params_str.join(", "))
            }
            Self::Class(name) => name.clone(),
            Self::View(inner, mutable) => {
                if *mutable {
                    format!("view mut {inner}")
                } else {
                    format!("view {inner}")
                }
            }
            Self::Shared(inner) => format!("shared {inner}"),
            Self::Any => "any".into(),
            Self::Pointer => "pointer".into(),
            Self::Slice(inner) => format!("slice[{inner}]"),
            Self::Own(inner) => format!("own {inner}"),
            Self::Ref(inner, mutable) => {
                format!("{}{}", if *mutable { "&mut " } else { "&" }, inner)
            }
            Self::RawPointer(inner, mutable) => {
                format!("{}{}", if *mutable { "*mut " } else { "*const " }, inner)
            }
            Self::Dyn(trait_name) => format!("dyn {trait_name}"),
        }
    }

    /// Whether a `view`/`view mut` may be taken of a value of this type.
    ///
    /// Views borrow heap-typed values. Scalars are plain values (copying
    /// them is free and invisible), so viewing them adds nothing, and a
    /// view cannot wrap another view.
    pub fn viewable(&self) -> bool {
        !matches!(
            self,
            Ty::Int | Ty::Float | Ty::Bool | Ty::Void | Ty::Nil | Ty::View(..)
        )
    }

    /// Returns true if this type is assignable from `other`.
    pub fn is_assignable_from(&self, other: &Ty) -> bool {
        match (self, other) {
            // Shared target: copy an existing shared reference, or adopt
            // an owned heap value (boxing it).
            (Self::Shared(a), Self::Shared(b)) => a.is_assignable_from(b),
            (Self::Shared(a), other) if other.viewable() => a.is_assignable_from(other),

            // A view target accepts a shared value by borrowing its
            // pointee.
            (Self::View(t1, _), Self::Shared(inner)) => t1.is_assignable_from(inner),

            // A shared value is never implicitly copied out to an owned
            // target (that would create a second owner of the pointee).
            (_, Self::Shared(_)) => false,

            // Trait-object matching needs the impl registry, so it is
            // layered on by `TypeChecker::assignable`, not here.
            (Self::Dyn(_), _) | (_, Self::Dyn(_)) => false,

            // Any accepts anything and is assignable to anything.
            (Self::Any, _) | (_, Self::Any) => true,
            (Self::Int, Self::Int) => true,
            (Self::Pointer, Self::Pointer) => true,
            (Self::Slice(a), Self::Slice(b)) => a.is_assignable_from(b) || matches!(**b, Self::Any),
            (Self::Own(a), Self::Own(b)) => a.is_assignable_from(b),
            (Self::Ref(a, am), Self::Ref(b, bm)) => (!*am || *bm) && a.is_assignable_from(b),
            (Self::RawPointer(a, am), Self::RawPointer(b, bm)) => {
                (!*am || *bm) && a.is_assignable_from(b)
            }

            // Implicit int → float widening.
            (Self::Float, Self::Int) => true,
            (Self::Float, Self::Float) => true,
            (Self::String, Self::String) => true,
            (Self::String, Self::Object) => true,
            (Self::Bool, Self::Bool) => true,
            (Self::Nil, Self::Nil) => true,
            (Self::Void, Self::Void) => true,
            (Self::Array(a), Self::Array(b)) => a.is_assignable_from(b),
            (Self::Option(a), Self::Option(b)) => a.is_assignable_from(b),
            (Self::Option(_), Self::Nil) => true,

            // A plain value auto-wraps into an `option[T]` slot:
            // assigning `5` to `var option[int] o` boxes the value.
            (Self::Option(a), b) if a.is_assignable_from(b) => true,
            (Self::Object, Self::Object) => true,
            (
                Self::Function {
                    params: p1,
                    return_type: r1,
                },
                Self::Function {
                    params: p2,
                    return_type: r2,
                },
            ) => {
                p1.len() == p2.len()
                    && p1
                        .iter()
                        .zip(p2.iter())
                        .all(|(a, b)| a.is_assignable_from(b))
                    && r1.is_assignable_from(r2)
            }
            (Self::Class(a), Self::Class(b)) => a == b,

            // A view-typed target accepts: another view (a shared target
            // takes shared or mutable views, a mutable target takes only
            // mutable views), or an owned value of the underlying type
            // (auto-view).
            (Self::View(t1, mut1), Self::View(t2, mut2)) => {
                (!*mut1 || *mut2) && t1.is_assignable_from(t2)
            }
            (Self::View(t1, _), other) => t1.is_assignable_from(other),

            // Reading through a view is always allowed: an owned target
            // is compatible with a view of it. (Storing a view is
            // rejected by the ownership checker, not the type relation.)
            (_, Self::View(t2, _)) => self.is_assignable_from(t2),
            _ => false,
        }
    }
}

impl fmt::Display for Ty {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.label())
    }
}
