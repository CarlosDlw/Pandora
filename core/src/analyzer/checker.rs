use std::collections::HashMap;

use crate::{
    hir::{BinOp, Hir, HirExpr, HirId, HirStmt, SymbolTable},
    analyzer::Type as AnalyzerType,
};
use foundation::{
    diagnostics::{Diagnostic, Diagnostics, Severity},
    span::Span,
};

#[derive(Debug, Default)]
pub struct SemanticModel {
    pub types: HashMap<HirId, AnalyzerType>,
}

pub fn analyze(hir: &Hir, symbols: &SymbolTable) -> (SemanticModel, Diagnostics) {
    let mut checker = Checker {
        hir,
        symbols,
        diagnostics: Diagnostics::new(),
        model: SemanticModel::default(),
    };
    checker.check_program();
    (checker.model, checker.diagnostics)
}

struct Checker<'a> {
    hir: &'a Hir,
    symbols: &'a SymbolTable,
    diagnostics: Diagnostics,
    model: SemanticModel,
}

impl<'a> Checker<'a> {
    fn check_program(&mut self) {
        for stmt in &self.hir.stmts {
            self.check_stmt(stmt);
        }
    }

    fn check_stmt(&mut self, stmt: &HirStmt) -> AnalyzerType {
        match stmt {
            HirStmt::Let { value, span, .. } => self.check_expr(*value, *span),
            HirStmt::Expr { expr, span } => self.check_expr(*expr, *span),
            HirStmt::Invalid { span } => {
                self.push_error("invalid statement", *span);
                AnalyzerType::Unknown
            }
        }
    }

    fn check_expr(&mut self, id: HirId, span: Span) -> AnalyzerType {
        if let Some(ty) = self.model.types.get(&id) {
            return ty.clone();
        }

        let ty = match self.hir.exprs.get(id) {
            Some(HirExpr::Int(_)) => AnalyzerType::Int,
            Some(HirExpr::Float(_)) => AnalyzerType::Float,
            Some(HirExpr::Bool(_)) => AnalyzerType::Bool,
            Some(HirExpr::Str(_)) => AnalyzerType::Str,
            Some(HirExpr::Var(symbol_id)) => self
                .symbols
                .symbol(*symbol_id)
                .map(|s| s.ty.clone())
                .unwrap_or(AnalyzerType::Unknown),
            Some(HirExpr::Binary { op, lhs, rhs }) => {
                let left_ty = self.check_expr(*lhs, span);
                let right_ty = self.check_expr(*rhs, span);
                self.check_binary(*op, left_ty, right_ty, span)
            }
            Some(HirExpr::Call { callee, args }) => {
                let callee_ty = self
                    .symbols
                    .symbol(*callee)
                    .map(|s| s.ty.clone())
                    .unwrap_or(AnalyzerType::Unknown);
                self.check_call(callee_ty, args, span)
            }
            Some(HirExpr::Invalid) | None => AnalyzerType::Unknown,
        };

        self.model.types.insert(id, ty.clone());
        ty
    }

    fn check_binary(
        &mut self,
        op: BinOp,
        left_ty: AnalyzerType,
        right_ty: AnalyzerType,
        span: Span,
    ) -> AnalyzerType {
        if left_ty == AnalyzerType::Int && right_ty == AnalyzerType::Int {
            return AnalyzerType::Int;
        }
        if left_ty == AnalyzerType::Float && right_ty == AnalyzerType::Float {
            return AnalyzerType::Float;
        }

        self.push_error(
            format!("invalid operands for {:?}: left={left_ty:?}, right={right_ty:?}", op),
            span,
        );
        AnalyzerType::Unknown
    }

    fn check_call(&mut self, callee_ty: AnalyzerType, args: &[HirId], span: Span) -> AnalyzerType {
        let AnalyzerType::Function { params, ret } = callee_ty else {
            self.push_error("attempted call on non-function value", span);
            for arg in args {
                let _ = self.check_expr(*arg, span);
            }
            return AnalyzerType::Unknown;
        };

        let is_variadic_any = params.len() == 1 && params[0] == AnalyzerType::Any;

        if !is_variadic_any && params.len() != args.len() {
            self.push_error(
                format!("invalid argument count: expected {}, got {}", params.len(), args.len()),
                span,
            );
        }

        for (idx, arg) in args.iter().enumerate() {
            let arg_ty = self.check_expr(*arg, span);
            let expected = if is_variadic_any {
                AnalyzerType::Any
            } else {
                params.get(idx).cloned().unwrap_or(AnalyzerType::Unknown)
            };
            if expected != AnalyzerType::Any
                && expected != AnalyzerType::Unknown
                && arg_ty != AnalyzerType::Unknown
                && arg_ty != expected
            {
                self.push_error(
                    format!("invalid argument type at position {idx}: expected {expected:?}, got {arg_ty:?}"),
                    span,
                );
            }
        }

        *ret
    }

    fn push_error(&mut self, message: impl Into<String>, span: Span) {
        self.diagnostics
            .push(Diagnostic::new(message, span, Severity::Error));
    }
}

#[cfg(test)]
mod tests {
    use foundation::ids::FileId;

    use crate::{lexer::lex, lowering::lower, parser::parse};

    use super::analyze;

    #[test]
    fn catches_invalid_call_argument_types() {
        let src = "len(1)";
        let lex_output = lex(FileId::from_u32(20), src);
        let (ast, _) = parse(FileId::from_u32(20), src.len() as u32, lex_output.tokens);
        let (hir, symbols, _) = lower(&ast);
        let (_model, diagnostics) = analyze(&hir, &symbols);
        assert!(diagnostics.has_errors());
    }

    #[test]
    fn validates_builtin_call_args() {
        let src = "print(1, 2)";
        let lex_output = lex(FileId::from_u32(21), src);
        let (ast, _) = parse(FileId::from_u32(21), src.len() as u32, lex_output.tokens);
        let (hir, symbols, _) = lower(&ast);
        let (_model, diagnostics) = analyze(&hir, &symbols);
        assert!(!diagnostics.has_errors());
    }
}
