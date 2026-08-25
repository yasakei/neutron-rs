//! Pattern exhaustiveness checking pass.
//!
//! Verifies that `match` statements cover all potential match patterns
//! or provide a fallback (`default` branch / wildcard pattern).

use crate::resolve::TypeError;
use ntsc_ast::pattern::Pattern;
use ntsc_ast::stmt::{Program, Stmt};

#[derive(Default)]
pub struct ExhaustivenessChecker {
    pub errors: Vec<TypeError>,
}

impl ExhaustivenessChecker {
    pub fn new() -> Self {
        Self { errors: Vec::new() }
    }

    pub fn check_program(&mut self, program: &Program) {
        for stmt in &program.statements {
            self.check_stmt(stmt);
        }
    }

    fn check_stmt(&mut self, stmt: &Stmt) {
        match stmt {
            Stmt::Match {
                expression,
                cases,
                default_case,
            } => {
                let has_fallback = default_case.is_some()
                    || cases
                        .iter()
                        .any(|c| c.guard.is_none() && self.is_catch_all(&c.pattern));

                if !has_fallback {
                    self.errors.push(TypeError {
                        code: None,
                        message: "match expression is not exhaustive (missing default branch or wildcard pattern)".to_string(),
                        span: expression.span(),
                    help: None,
                    });
                }

                for c in cases {
                    self.check_stmt(&c.body);
                }
                if let Some(def) = default_case {
                    self.check_stmt(def);
                }
            }
            Stmt::Block { statements, .. } => {
                for s in statements {
                    self.check_stmt(s);
                }
            }
            Stmt::If {
                then_branch,
                elif_branches,
                else_branch,
                ..
            } => {
                self.check_stmt(then_branch);
                for elif in elif_branches {
                    self.check_stmt(&elif.body);
                }
                if let Some(else_b) = else_branch {
                    self.check_stmt(else_b);
                }
            }
            Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => {
                self.check_stmt(body);
            }
            Stmt::For { body, .. } | Stmt::ForIn { body, .. } | Stmt::ForAwait { body, .. } => {
                self.check_stmt(body);
            }
            Stmt::Function { body, .. } => {
                for s in body {
                    self.check_stmt(s);
                }
            }
            Stmt::Class { body, .. } => {
                for s in body {
                    self.check_stmt(s);
                }
            }
            Stmt::Try {
                try_block,
                catch_block,
                finally_block,
                ..
            } => {
                self.check_stmt(try_block);
                if let Some(c) = catch_block {
                    self.check_stmt(c);
                }
                if let Some(f) = finally_block {
                    self.check_stmt(f);
                }
            }
            Stmt::Retry {
                body, catch_block, ..
            } => {
                self.check_stmt(body);
                if let Some(c) = catch_block {
                    self.check_stmt(c);
                }
            }
            Stmt::Unsafe { body } => {
                self.check_stmt(body);
            }
            Stmt::Quiet { body, .. } => {
                self.check_stmt(body);
            }
            _ => {}
        }
    }

    fn is_catch_all(&self, pattern: &Pattern) -> bool {
        matches!(pattern, Pattern::Wildcard { .. } | Pattern::Variable { .. })
    }
}
