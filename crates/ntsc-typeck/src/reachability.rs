//! Unreachable code detection pass.
//!
//! Scans AST blocks and statement sequences for code occurring after
//! terminating statements (`return`, `throw`, `break`, `continue`).

use crate::resolve::TypeError;
use ntsc_ast::span::Span;
use ntsc_ast::stmt::{Program, Stmt};

#[derive(Default)]
pub struct ReachabilityChecker {
    pub errors: Vec<TypeError>,
}

impl ReachabilityChecker {
    pub fn new() -> Self {
        Self { errors: Vec::new() }
    }

    pub fn check_program(&mut self, program: &Program) {
        self.check_stmts(&program.statements);
    }

    fn check_stmts(&mut self, stmts: &[Stmt]) {
        let mut terminated = false;
        let mut term_span = None;

        for stmt in stmts {
            if terminated {
                if let Some(span) = term_span {
                    self.errors.push(TypeError {
                        code: None,
                        message: "unreachable code after terminal statement".to_string(),
                        span,
                    help: None,
                    });
                }
                break;
            }

            self.check_stmt(stmt);

            if self.is_terminating(stmt) {
                terminated = true;
                term_span = Some(self.stmt_span(stmt));
            }
        }
    }

    fn check_stmt(&mut self, stmt: &Stmt) {
        match stmt {
            Stmt::Block { statements, .. } => {
                self.check_stmts(statements);
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
                self.check_stmts(body);
            }
            Stmt::Class { body, .. } => {
                self.check_stmts(body);
            }
            Stmt::Match {
                cases,
                default_case,
                ..
            } => {
                for c in cases {
                    self.check_stmt(&c.body);
                }
                if let Some(def) = default_case {
                    self.check_stmt(def);
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

    fn is_terminating(&self, stmt: &Stmt) -> bool {
        match stmt {
            Stmt::Return { .. }
            | Stmt::Throw { .. }
            | Stmt::Break { .. }
            | Stmt::Continue { .. } => true,
            Stmt::Block { statements, .. } => {
                statements.last().map_or(false, |s| self.is_terminating(s))
            }
            Stmt::If {
                then_branch,
                elif_branches,
                else_branch,
                ..
            } => {
                if let Some(else_b) = else_branch {
                    self.is_terminating(then_branch)
                        && elif_branches.iter().all(|e| self.is_terminating(&e.body))
                        && self.is_terminating(else_b)
                } else {
                    false
                }
            }
            Stmt::Quiet { body, .. } => self.is_terminating(body),
            _ => false,
        }
    }

    fn stmt_span(&self, stmt: &Stmt) -> Span {
        match stmt {
            Stmt::Return { value } => value.as_ref().map(|e| e.span()).unwrap_or_else(Span::dummy),
            Stmt::Throw { value } => value.span(),
            Stmt::Break { span } | Stmt::Continue { span } => *span,
            _ => Span::dummy(),
        }
    }
}
