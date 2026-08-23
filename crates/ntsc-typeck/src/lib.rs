//! Type checker for the NTSC language.
//!
//! Performs name resolution and bidirectional type checking on the parsed AST.

mod diag;
mod generics;
mod names;
mod ownership;
mod resolve;
mod scope;
mod ty;
mod warnings;

pub use generics::{TraitMethodInfo, TraitObjectInfo, prepare_program, take_trait_object_tables};
pub use names::{ResolveError, resolve_program};
pub use resolve::{TypeError, check_program};
pub use ty::Ty;
pub use warnings::{Warning, lint_program};
