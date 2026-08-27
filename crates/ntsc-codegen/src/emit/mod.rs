//! LLVM IR emission for NTSC.
//!
//! The emitter is split by concern; each submodule self-contains its section:
//! `runtime` (extern declarations), `typing` (type mapping), `module`
//! (entry points), `lookup` + `escape` + `function` (function metadata and
//! stack analysis), `async_sm` (state machines), `class`/`stmt`/`expr`/
//! `literal`/`binary`/`member`/`call`/`assign`/`control`/`exception` (language
//! constructs), `helper` (shared conversions), `array` (RC-backed ops),
//! `drop` (owned-value reclaim), `c_main` (the generated main), and `tests`.

use std::cell::RefCell;
use std::collections::HashMap;
use std::collections::HashSet;
use std::hash::{Hash, Hasher};

use inkwell::AddressSpace;
use inkwell::IntPredicate;
use inkwell::builder::Builder;
use inkwell::context::Context;
use inkwell::module::Module;
use inkwell::types::{BasicMetadataTypeEnum, BasicType};

thread_local! {
    /// Class name → ordered field names (declaration order).
    static CLASS_FIELDS: RefCell<HashMap<String, Vec<String>>> = RefCell::new(HashMap::new());
}

thread_local! {
    /// Class name → ordered field types.
    ///
    /// Mirrors `CLASS_FIELDS` so member access can recover the exact type of
    /// an array/class field instead of reverse-mapping the LLVM type, which
    /// collapses every pointer field (arrays, strings, class instances) to
    /// `String`.
    static CLASS_FIELD_TYPES: RefCell<HashMap<String, Vec<Ty>>> = RefCell::new(HashMap::new());
}

thread_local! {
    /// Class name → per-field declared initializer (`None` when undeclared).
    ///
    /// Initializers run at construction, before `init`, so a constructor can
    /// still overwrite them.
    static CLASS_FIELD_INITS: RefCell<HashMap<String, Vec<Option<Expr>>>> =
        RefCell::new(HashMap::new());
}

thread_local! {
    /// Classes whose deep copy is currently being emitted.
    ///
    /// A class that (directly or transitively) holds a field of its own type
    /// would otherwise make copy recursion overflow the compiler stack;
    /// re-entry is reported as a codegen error instead.
    static CLASS_COPY_IN_PROGRESS: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
}

thread_local! {
    /// Class name → `extends` base class name.
    ///
    /// Used to resolve inherited members and to lay out derived structs with
    /// the base fields first, so parent methods see a layout-compatible
    /// instance.
    static CLASS_PARENTS: RefCell<HashMap<String, String>> = RefCell::new(HashMap::new());
}

thread_local! {
    /// Stdlib import alias → real module name, from `use strings as s`.
    ///
    /// The alias is bound as an object in the source, but native stdlib
    /// functions are named `ntsc_{module}_{fn}`, so codegen translates an
    /// aliased member call back to the real module name.
    static STDLIB_ALIASES: RefCell<HashMap<String, String>> = RefCell::new(HashMap::new());
}

thread_local! {
    /// Enum name → member name → i32 value. Enums are lowered to int
    /// constants; `Color.RED` resolves through this table.
    static ENUM_VALUES: RefCell<HashMap<String, HashMap<String, i32>>> =
        RefCell::new(HashMap::new());
}

thread_local! {
    /// Bare enum member name → i32 value.
    ///
    /// Allows `case North` and `say(North)` where the member is referenced
    /// without its enum qualifier; last declaration in program order wins.
    static ENUM_MEMBER_VALUES: RefCell<HashMap<String, i32>> = RefCell::new(HashMap::new());
}

thread_local! {
    /// `static const` variable name → declared type.
    ///
    /// Used by `emit_variable` to type a reference to a module-level
    /// constant emitted as an LLVM global.
    static STATIC_CONST_TYPES: RefCell<HashMap<String, Ty>> = RefCell::new(HashMap::new());
}

thread_local! {
    /// `static const` variable name → its literal initializer (kept for
    /// lazily building string handles on first use).
    static STATIC_CONST_INITS: RefCell<HashMap<String, Option<Expr>>> =
        RefCell::new(HashMap::new());
}

thread_local! {
    /// `static const` variable name → pre-evaluated compile-time value,
    /// populated by the type checker's const evaluator.
    static CONST_EVAL_VALUES: RefCell<HashMap<String, ntsc_typeck::ConstValue>> =
        RefCell::new(HashMap::new());
}

thread_local! {
    /// Class name → method name → declared return type.
    ///
    /// Used by the for-in iterator protocol to type the loop variable from
    /// the `get(i)` method's declared return type.
    static CLASS_METHOD_TYPES: RefCell<HashMap<String, HashMap<String, Ty>>> =
        RefCell::new(HashMap::new());
}

thread_local! {
    /// User-function name → declared return type.
    ///
    /// Call sites type the result from the annotation instead of inferring
    /// from the LLVM pointer return type, which would map any class pointer
    /// to `String`.
    static FUNCTION_RETURN_TYPES: RefCell<HashMap<String, Ty>> = RefCell::new(HashMap::new());
}

thread_local! {
    /// User-function name → declared parameter types.
    ///
    /// Decides whether a call argument is moved (owned parameter) or borrowed
    /// (view parameter).
    static FUNCTION_PARAM_TYPES: RefCell<HashMap<String, Vec<Ty>>> = RefCell::new(HashMap::new());
}

thread_local! {
    /// Class name → method name → declared parameter types (excluding `this`).
    static CLASS_METHOD_PARAM_TYPES: RefCell<HashMap<String, HashMap<String, Vec<Ty>>>> =
        RefCell::new(HashMap::new());
}

use inkwell::values::{BasicMetadataValueEnum, BasicValueEnum, FunctionValue, PointerValue};

use ntsc_ast::expr::{Expr, LiteralValue};
use ntsc_ast::stmt::{Program, Stmt};
use ntsc_ast::token::TokenKind;
use ntsc_typeck::Ty;

/// Evaluate an expression that must be a compile-time int constant (used for
/// explicit `enum Member = value` values). Returns `None` for anything else.
fn const_int_expr_value(expr: &Expr) -> Option<i32> {
    match expr {
        Expr::Literal {
            value: LiteralValue::Number(n),
            ..
        } => n.parse::<i64>().ok().and_then(|v| i32::try_from(v).ok()),
        Expr::Unary { op, right } if op.lexeme() == "-" => {
            const_int_expr_value(right).map(|v| v.wrapping_neg())
        }
        Expr::Grouping { expression, .. } => const_int_expr_value(expression),
        _ => None,
    }
}

// ── Type-tracked value ──────────────────────────────────────────────────

#[derive(Clone)]
/// A value with its NTSC type information, keeping the language-level type
/// alongside the LLVM representation.
pub(crate) struct TypedValue<'ctx> {
    value: BasicValueEnum<'ctx>,
    ntsc_type: Ty,
}

impl<'ctx> TypedValue<'ctx> {
    fn new(value: BasicValueEnum<'ctx>, ntsc_type: Ty) -> Self {
        Self { value, ntsc_type }
    }
}

// ── Per-function state ──────────────────────────────────────────────────

pub(crate) struct FunctionContext<'ctx, 'm> {
    function: FunctionValue<'ctx>,
    builder: &'m Builder<'ctx>,
    entry_builder: &'m Builder<'ctx>,
    entry_bb: inkwell::basic_block::BasicBlock<'ctx>,
    module: &'m Module<'ctx>,
    variables: HashMap<String, (PointerValue<'ctx>, Ty)>,
    return_type: Ty,
    context: &'ctx Context,
    break_targets: Vec<inkwell::basic_block::BasicBlock<'ctx>>,
    continue_targets: Vec<inkwell::basic_block::BasicBlock<'ctx>>,
    loop_owned_slots: Vec<HashSet<String>>,

    /// Exception-handler blocks of enclosing `try`/`retry`, innermost last.
    /// A pending exception branches to the innermost block; when empty it
    /// unwinds to the function's exception-return block.
    exception_targets: Vec<inkwell::basic_block::BasicBlock<'ctx>>,

    /// The block a pending exception branches to when no enclosing
    /// `try`/`retry` handles it: it drops all owned locals and returns a
    /// default value. Created lazily on first use.
    exception_return_bb: Option<inkwell::basic_block::BasicBlock<'ctx>>,

    /// Whether calls emit pending-exception checks. Disabled inside async
    /// poll functions, whose state machines have no exception support.
    exception_checks: bool,

    /// Bitcast future struct pointer and its struct type when emitting an
    /// async poll function (top-level locals live in its fields, so they
    /// survive suspension).
    future_base: Option<(PointerValue<'ctx>, inkwell::types::StructType<'ctx>)>,

    /// Field index per async local name. When present, `alloca` becomes a GEP
    /// into the future struct (a deterministic field, stable across the
    /// resumption of each segment) instead of a stack alloca.
    async_fields: Option<HashMap<String, u32>>,

    /// Locals that escape analysis proved can be stack-allocated instead of
    /// heap-allocated.
    stack_allocated: HashSet<String>,

    /// Local `var x = ClassName(...)` constructions whose instance never
    /// escapes or is aliased within the function. Their owned fields are
    /// reclaimed by the class drop thunk at scope exit.
    class_drops: HashSet<String>,

    /// Names of `var`-declared locals (or owned parameters) whose static
    /// type is a heap array or heap string: the slot is zero-initialized,
    /// the previous value is dropped before an overwrite, and the current
    /// value is dropped at function exit. Moving an owned value into another
    /// slot nulls the source so it is never dropped twice. View parameters
    /// and `for-in` loop variables are borrowed and never appear here.
    owned_slots: HashSet<String>,

    /// Owned slots that a re-declaration or block exit displaced, so they can
    /// no longer be reached through `variables`. Their values are still live
    /// and dropped at function exit alongside the slots still bound by name.
    shadowed_owned_slots: Vec<(PointerValue<'ctx>, Ty)>,

    /// Maps `Expr::AsyncBlock` spans to their generated anonymous function
    /// names. Populated during async poll emission for standalone blocks and
    /// `wait_any`/`wait_all` argument resolution.
    pub(crate) block_span_to_name: Option<HashMap<usize, String>>,
}

impl<'ctx, 'm> FunctionContext<'ctx, 'm> {
    fn new(
        function: FunctionValue<'ctx>,
        builder: &'m Builder<'ctx>,
        entry_builder: &'m Builder<'ctx>,
        entry_bb: inkwell::basic_block::BasicBlock<'ctx>,
        module: &'m Module<'ctx>,
        return_type: Ty,
        context: &'ctx Context,
    ) -> Self {
        Self {
            function,
            builder,
            entry_builder,
            entry_bb,
            module,
            variables: HashMap::new(),
            return_type,
            context,
            break_targets: Vec::new(),
            continue_targets: Vec::new(),
            loop_owned_slots: Vec::new(),
            exception_targets: Vec::new(),
            exception_return_bb: None,
            exception_checks: true,
            future_base: None,
            async_fields: None,
            stack_allocated: HashSet::new(),
            class_drops: HashSet::new(),
            owned_slots: HashSet::new(),
            shadowed_owned_slots: Vec::new(),
            block_span_to_name: None,
        }
    }

    fn current_exception_handler(&mut self) -> inkwell::basic_block::BasicBlock<'ctx> {
        // The innermost enclosing `try`/`retry` handler, or the function-level
        // exception-return block, created on first use.
        if let Some(bb) = self.exception_targets.last() {
            return *bb;
        }
        if let Some(bb) = self.exception_return_bb {
            return bb;
        }
        let bb = self
            .context
            .append_basic_block(self.function, "exception.return");
        self.exception_return_bb = Some(bb);
        bb
    }

    /// Check the runtime's pending-exception flag after a call that may
    /// throw: on pending, branch to the current exception handler; otherwise
    /// continue in a fresh block so the flag is not re-read by later
    /// instructions. A no-op inside async state machines.
    fn emit_pending_exception_check(&mut self) -> Result<(), crate::CodegenError> {
        if !self.exception_checks {
            return Ok(());
        }
        let handler = self.current_exception_handler();
        let pending_fn = self
            .module
            .get_function("ntsc_exception_pending")
            .ok_or_else(|| {
                crate::CodegenError::LLVMError("ntsc_exception_pending not declared".into())
            })?;
        let pending = self
            .builder
            .build_call(pending_fn, &[], "exc_pending")?
            .try_as_basic_value()
            .unwrap_basic()
            .into_int_value();
        let active = self.builder.build_int_compare(
            inkwell::IntPredicate::NE,
            pending,
            self.context.i8_type().const_zero(),
            "exc_active",
        )?;
        let continue_bb = self
            .context
            .append_basic_block(self.function, "exc.continue");
        self.builder
            .build_conditional_branch(active, handler, continue_bb)?;
        self.builder.position_at_end(continue_bb);
        Ok(())
    }

    fn push_loop_targets(
        &mut self,
        break_target: inkwell::basic_block::BasicBlock<'ctx>,
        continue_target: inkwell::basic_block::BasicBlock<'ctx>,
    ) {
        self.break_targets.push(break_target);
        self.continue_targets.push(continue_target);
        self.loop_owned_slots.push(self.owned_slots.clone());
    }

    fn pop_loop_targets(&mut self) {
        self.break_targets.pop();
        self.continue_targets.pop();
        self.loop_owned_slots.pop();
    }

    fn drop_loop_locals(&mut self) -> Result<(), crate::CodegenError> {
        let Some(before) = self.loop_owned_slots.last() else {
            return Ok(());
        };
        let mut names: Vec<_> = self.owned_slots.difference(before).cloned().collect();
        names.sort();
        for name in names {
            if let Some((ptr, ty)) = self
                .variables
                .get(&name)
                .map(|(ptr, ty)| (*ptr, ty.clone()))
            {
                emit_drop_slot_value(self, ptr, &ty)?;
                self.builder
                    .build_store(ptr, default_llvm_value(&ty, self.context))?;
            }
        }
        Ok(())
    }

    fn emit_break(&mut self) -> Result<(), crate::CodegenError> {
        let target = match self.break_targets.last() {
            Some(target) => *target,
            None => {
                return Err(crate::CodegenError::LLVMError(
                    "`break` outside of a loop".into(),
                ));
            }
        };
        self.drop_loop_locals()?;
        self.builder.build_unconditional_branch(target)?;
        Ok(())
    }

    fn emit_continue(&mut self) -> Result<(), crate::CodegenError> {
        let target = match self.continue_targets.last() {
            Some(target) => *target,
            None => {
                return Err(crate::CodegenError::LLVMError(
                    "`continue` outside of a loop".into(),
                ));
            }
        };
        self.drop_loop_locals()?;
        self.builder.build_unconditional_branch(target)?;
        Ok(())
    }

    /// Allocate space for a local variable at the very start of the entry block.
    ///
    /// The alloca is inserted before the entry block's first instruction so
    /// it is never placed after a terminator (e.g. when a variable is
    /// declared inside a loop body). Inside an async poll function, a
    /// top-level local is instead a field of the future struct (a GEP), so it
    /// survives suspension.
    fn alloca(&mut self, name: &str, ty: &Ty) -> Result<PointerValue<'ctx>, crate::CodegenError> {
        if let Some(fields) = &self.async_fields {
            let index = fields.get(name).copied().ok_or_else(|| {
                crate::CodegenError::LLVMError(format!(
                    "internal: async local `{name}` has no future field"
                ))
            })?;
            return self.future_field(index);
        }
        let llvm_ty = ty_to_llvm(ty, self.context);
        match self.entry_bb.get_first_instruction() {
            Some(first) => self.entry_builder.position_before(&first),
            None => self.entry_builder.position_at_end(self.entry_bb),
        }
        let slot = self.entry_builder.build_alloca(llvm_ty, name)?;

        // Every slot a drop path can read starts null, so dropping a slot
        // that was never assigned is a safe no-op. Leaving a kind out of
        // `ty_is_owned_handle` would leave its slot `undef` until the first
        // store: a debug build reads whatever a fresh stack page holds, but
        // `mem2reg` turns the pre-store load into poison and the drop
        // thunk's null check can fold either way, freeing garbage handles.
        if ty_is_owned_handle(ty) {
            let zero = default_llvm_value(ty, self.context);
            self.entry_builder.build_store(slot, zero)?;
        }
        Ok(slot)
    }

    /// Allocate a raw LLVM-typed stack slot in the entry block. Used by
    /// escape analysis to place a stack-allocated class object; not used
    /// inside async poll functions.
    fn alloca_llvm(
        &mut self,
        name: &str,
        llvm_ty: inkwell::types::BasicTypeEnum<'ctx>,
    ) -> Result<PointerValue<'ctx>, crate::CodegenError> {
        match self.entry_bb.get_first_instruction() {
            Some(first) => self.entry_builder.position_before(&first),
            None => self.entry_builder.position_at_end(self.entry_bb),
        }
        Ok(self.entry_builder.build_alloca(llvm_ty, name)?)
    }

    fn future_field(&self, index: u32) -> Result<PointerValue<'ctx>, crate::CodegenError> {
        let (base, struct_ty) = self.future_base.ok_or_else(|| {
            crate::CodegenError::LLVMError("future field access outside an async poll".into())
        })?;
        self.builder
            .build_struct_gep(struct_ty, base, index, "future_field")
            .map_err(|e| crate::CodegenError::LLVMError(format!("future field GEP: {e}")))
    }

    /// Define a new variable in the current scope.
    ///
    /// A re-declaration in an inner block takes over the name, leaving the
    /// slot it displaced unreachable by name; if that slot owned a heap
    /// value it moves to `shadowed_owned_slots` so the exit-time drop still
    /// reclaims it instead of leaking it.
    fn define_var(&mut self, name: &str, ptr: PointerValue<'ctx>, ty: Ty) {
        if let Some((old_ptr, old_ty)) = self.variables.get(name)
            && *old_ptr != ptr
            && self.owned_slots.contains(name)
        {
            self.shadowed_owned_slots.push((*old_ptr, old_ty.clone()));
        }
        self.variables.insert(name.to_string(), (ptr, ty));
    }

    /// The bindings currently in scope, handed back to `end_block_scope`.
    fn begin_block_scope(&self) -> HashMap<String, (PointerValue<'ctx>, Ty)> {
        self.variables.clone()
    }

    /// Leave a block: names it declared go out of scope and the bindings they
    /// displaced become visible again. The slots themselves outlive the block
    /// (entry-block allocas) and their values are reclaimed at function exit,
    /// so an owned slot leaving scope moves to `shadowed_owned_slots`
    /// (unreachable by name, still dropped) exactly like one a re-declaration
    /// displaced.
    fn end_block_scope(&mut self, outer: HashMap<String, (PointerValue<'ctx>, Ty)>) {
        let declared: Vec<(String, PointerValue<'ctx>, Ty)> = self
            .variables
            .iter()
            .filter(|(name, (ptr, _))| outer.get(*name).is_none_or(|(o, _)| o != ptr))
            .map(|(name, (ptr, ty))| (name.clone(), *ptr, ty.clone()))
            .collect();
        for (name, ptr, ty) in declared {
            if !self.owned_slots.contains(&name) {
                continue;
            }
            self.shadowed_owned_slots.push((ptr, ty));
            match outer.get(&name) {
                Some((outer_ptr, _)) => {
                    // The declaration displaced an outer slot of the same
                    // name (`define_var` moved it to `shadowed_owned_slots`);
                    // it is reachable by name again, so drop it through the
                    // name and leave exactly one owner for each slot.
                    if let Some(at) = self
                        .shadowed_owned_slots
                        .iter()
                        .position(|(p, _)| p == outer_ptr)
                    {
                        self.shadowed_owned_slots.remove(at);
                    }
                }

                None => {
                    // Nothing outside the block owns this name any more.
                    self.owned_slots.remove(&name);
                }
            }
        }
        self.variables = outer;
    }

    fn lookup_var(&self, name: &str) -> Option<(PointerValue<'ctx>, &Ty)> {
        self.variables.get(name).map(|(ptr, ty)| (*ptr, ty))
    }

    /// Register `name` as an owned slot if its type owns a heap allocation
    /// (array, string, object, or shared box). View parameters, `this`, and
    /// scalars are never owned. Class slots are owned only when escape
    /// analysis proved the instance is not aliased; a borrowed class value
    /// (an element read, a field read) must never be dropped.
    fn mark_owned_if_heap(&mut self, name: &str, ty: &Ty) {
        let heap_owned = matches!(
            ty,
            Ty::Array(_)
                | Ty::String
                | Ty::Object
                | Ty::Shared(_)
                | Ty::Option(_)
                | Ty::Result { .. }
                | Ty::Pointer
                | Ty::Slice(_)
                | Ty::Own(_)
                | Ty::Dyn(_)
        ) || (matches!(ty, Ty::Class(_)) && self.class_drops.contains(name));
        if heap_owned {
            self.owned_slots.insert(name.to_string());
        }
    }

    /// Null the stack slot of an owned variable after its value was moved
    /// elsewhere, so the exit-time drop is a no-op. Shared boxes are never
    /// moved (only retained) and class values use reference semantics
    /// (`var y = x` aliases `x`), so those slots are left untouched. Option
    /// and result slots *are* nulled: both are owned cells, so a move that
    /// left the slot intact would let both the destination and this scope's
    /// exit free the same cell. A `dyn` fat pointer owns its header, so it
    /// is nulled on move for the same reason.
    fn null_var_slot(&mut self, name: &str) {
        if let Some((ptr, ty)) = self.lookup_var(name)
            && matches!(
                ty,
                Ty::Array(_)
                    | Ty::String
                    | Ty::Object
                    | Ty::Option(_)
                    | Ty::Dyn(_)
                    | Ty::Result { .. }
            )
        {
            let _ = self
                .builder
                .build_store(ptr, default_llvm_value(ty, self.context));
        }
    }
}

pub(crate) mod array;
pub(crate) mod assign;
pub(crate) mod async_sm;
pub(crate) mod binary;
pub(crate) mod c_main;
pub(crate) mod call;
pub(crate) mod class;
pub(crate) mod control;
pub(crate) mod drop;
pub(crate) mod dyn_obj;
pub(crate) mod escape;
pub(crate) mod exception;
pub(crate) mod expr;
pub(crate) mod function;
pub(crate) mod helper;
pub(crate) mod literal;
pub(crate) mod lookup;
pub(crate) mod member;
pub(crate) mod module;
pub(crate) mod result_cell;
pub(crate) mod runtime;
pub(crate) mod stmt;
#[cfg(test)]
pub(crate) mod tests;
pub(crate) mod typing;

pub(crate) use array::*;
pub(crate) use assign::*;
pub(crate) use async_sm::*;
pub(crate) use binary::*;
pub(crate) use c_main::*;
pub(crate) use call::*;
pub(crate) use class::*;
pub(crate) use control::*;
pub(crate) use drop::*;
pub(crate) use escape::*;
pub(crate) use exception::*;
pub(crate) use expr::*;
pub(crate) use function::*;
pub(crate) use helper::*;
pub(crate) use literal::*;
pub(crate) use lookup::*;
pub(crate) use member::*;
pub use module::emit_module;
pub(crate) use result_cell::*;
pub(crate) use runtime::*;
pub(crate) use stmt::*;
pub(crate) use typing::*;
