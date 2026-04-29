use std::collections::HashMap;

use crate::{
    analyzer::Type as AnalyzerType,
    hir::{BinOp, Hir, HirExpr, HirId, HirStmt, SymbolTable, UnaryOp},
    integer_lit::{literal_f64, literal_u128},
};
use foundation::{
    diagnostics::{Diagnostic, Diagnostics, Severity},
    span::Span,
};

#[derive(Debug, Default)]
pub struct SemanticModel {
    pub types: HashMap<HirId, AnalyzerType>,
}

pub fn analyze(hir: &Hir, symbols: &mut SymbolTable) -> (SemanticModel, Diagnostics) {
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
    symbols: &'a mut SymbolTable,
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
            HirStmt::Let {
                symbol,
                value,
                span,
                ..
            } => {
                let declared = self
                    .symbols
                    .symbol(*symbol)
                    .map(|s| s.ty.clone())
                    .unwrap_or(AnalyzerType::Unknown);
                let actual = self.check_expr_expected(*value, *span, Some(&declared));
                let final_ty = if matches!(declared, AnalyzerType::Unknown) {
                    actual.clone()
                } else {
                    declared.clone()
                };
                if let Some(sym) = self.symbols.symbol_mut(*symbol) {
                    sym.ty = final_ty.clone();
                }

                if !is_assignable(&final_ty, &actual) {
                    self.push_error(
                        format!("cannot assign value of type {actual:?} to {final_ty:?}"),
                        *span,
                    );
                }
                final_ty
            }
            HirStmt::Expr { expr, span } => self.check_expr(*expr, *span),
            HirStmt::Assign {
                symbol,
                value,
                span,
            } => {
                let expected = self
                    .symbols
                    .symbol(*symbol)
                    .map(|s| (s.ty.clone(), s.is_const))
                    .unwrap_or((AnalyzerType::Unknown, false));
                if expected.1 {
                    self.push_error("cannot assign to constant", *span);
                }
                let actual = self.check_expr_expected(*value, *span, Some(&expected.0));
                if !is_assignable(&expected.0, &actual) {
                    self.push_error(
                        format!("cannot assign value of type {actual:?} to {:?}", expected.0),
                        *span,
                    );
                }
                expected.0
            }
            HirStmt::Block { stmts, .. } => {
                for stmt in stmts {
                    let _ = self.check_stmt(stmt);
                }
                AnalyzerType::Unknown
            }
            HirStmt::If {
                condition,
                then_branch,
                else_branch,
                span,
            } => {
                let cond_ty = self.check_expr(*condition, *span);
                if !is_truthy_falsy_compatible(&cond_ty) && cond_ty != AnalyzerType::Unknown {
                    self.push_error(
                        format!("if condition is not truthy/falsy-compatible: {cond_ty:?}"),
                        *span,
                    );
                }
                for stmt in then_branch {
                    let _ = self.check_stmt(stmt);
                }
                if let Some(else_stmts) = else_branch {
                    for stmt in else_stmts {
                        let _ = self.check_stmt(stmt);
                    }
                }
                AnalyzerType::Unknown
            }
            HirStmt::Invalid { span } => {
                self.push_error("invalid statement", *span);
                AnalyzerType::Unknown
            }
        }
    }

    fn check_expr(&mut self, id: HirId, span: Span) -> AnalyzerType {
        self.check_expr_expected(id, span, None)
    }

    fn check_expr_expected(
        &mut self,
        id: HirId,
        span: Span,
        expected: Option<&AnalyzerType>,
    ) -> AnalyzerType {
        if let Some(ty) = self.model.types.get(&id) {
            return ty.clone();
        }

        let ty = match self.hir.exprs.get(id) {
            Some(HirExpr::Int(raw)) => self.check_int_literal(raw, expected, span),
            Some(HirExpr::Float(raw)) => self.check_float_literal(raw, expected, span),
            Some(HirExpr::Bool(_)) => AnalyzerType::Bool,
            Some(HirExpr::Str(_)) => AnalyzerType::Str,
            Some(HirExpr::Char(_)) => AnalyzerType::Char,
            Some(HirExpr::Var(symbol_id)) => self
                .symbols
                .symbol(*symbol_id)
                .map(|s| s.ty.clone())
                .unwrap_or(AnalyzerType::Unknown),
            Some(HirExpr::Unary {
                op: UnaryOp::Neg,
                operand,
            }) => {
                let operand_ty = self.check_expr(*operand, span);
                self.check_unary_neg(operand_ty, span)
            }
            Some(HirExpr::Unary {
                op: UnaryOp::Not,
                operand,
            }) => {
                let operand_ty = self.check_expr(*operand, span);
                self.check_unary_not(operand_ty, span)
            }
            Some(HirExpr::Unary {
                op: UnaryOp::BitNot,
                operand,
            }) => {
                let operand_ty = self.check_expr(*operand, span);
                self.check_unary_bit_not(operand_ty, span)
            }
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

    fn check_unary_neg(&mut self, operand_ty: AnalyzerType, span: Span) -> AnalyzerType {
        match operand_ty {
            AnalyzerType::Int { signed: true, bits } => AnalyzerType::Int { signed: true, bits },
            AnalyzerType::Float { bits } => AnalyzerType::Float { bits },
            AnalyzerType::Int {
                signed: false,
                bits,
            } => {
                self.push_error(
                    format!("unary '-' is invalid for unsigned integer u{bits}"),
                    span,
                );
                AnalyzerType::Unknown
            }
            other => {
                self.push_error(
                    format!("unary '-' expects numeric operand, got {other:?}"),
                    span,
                );
                AnalyzerType::Unknown
            }
        }
    }

    fn check_unary_not(&mut self, operand_ty: AnalyzerType, span: Span) -> AnalyzerType {
        match operand_ty {
            AnalyzerType::Bool => AnalyzerType::Bool,
            other => {
                self.push_error(
                    format!("logical '!' expects bool operand, got {other:?}"),
                    span,
                );
                AnalyzerType::Unknown
            }
        }
    }

    fn check_unary_bit_not(&mut self, operand_ty: AnalyzerType, span: Span) -> AnalyzerType {
        match operand_ty {
            AnalyzerType::Int { signed, bits } => AnalyzerType::Int { signed, bits },
            other => {
                self.push_error(
                    format!("bitwise '~' expects integer operand, got {other:?}"),
                    span,
                );
                AnalyzerType::Unknown
            }
        }
    }

    fn check_binary(
        &mut self,
        op: BinOp,
        left_ty: AnalyzerType,
        right_ty: AnalyzerType,
        span: Span,
    ) -> AnalyzerType {
        match op {
            BinOp::Add | BinOp::Subtract | BinOp::Multiply | BinOp::Divide | BinOp::Modulo | BinOp::Power => {
                self.check_numeric_pair(op, left_ty, right_ty, span)
            }
            BinOp::Equal | BinOp::NotEqual => self.check_equality_pair(op, left_ty, right_ty, span),
            BinOp::Less | BinOp::LessEqual | BinOp::Greater | BinOp::GreaterEqual => {
                let ty = self.check_numeric_pair(op, left_ty, right_ty, span);
                if matches!(ty, AnalyzerType::Unknown) {
                    AnalyzerType::Unknown
                } else {
                    AnalyzerType::Bool
                }
            }
            BinOp::LogicalAnd | BinOp::LogicalOr => {
                if left_ty == AnalyzerType::Bool && right_ty == AnalyzerType::Bool {
                    AnalyzerType::Bool
                } else {
                    self.push_error(
                        format!("logical operator {:?} expects bool operands, got left={left_ty:?}, right={right_ty:?}", op),
                        span,
                    );
                    AnalyzerType::Unknown
                }
            }
            BinOp::ShiftLeft | BinOp::ShiftRight => match (&left_ty, &right_ty) {
                (AnalyzerType::Int { .. }, AnalyzerType::Int { .. }) => left_ty,
                _ => {
                    self.push_error(
                        format!("shift operator {:?} expects integer operands, got left={left_ty:?}, right={right_ty:?}", op),
                        span,
                    );
                    AnalyzerType::Unknown
                }
            },
            BinOp::BitAnd | BinOp::BitOr | BinOp::BitXor => {
                match (&left_ty, &right_ty) {
                    (
                        AnalyzerType::Int {
                            signed: ls,
                            bits: lb,
                        },
                        AnalyzerType::Int {
                            signed: rs,
                            bits: rb,
                        },
                    ) if ls == rs && lb == rb => left_ty,
                    _ => {
                        self.push_error(
                            format!("bitwise operator {:?} expects matching integer operands, got left={left_ty:?}, right={right_ty:?}", op),
                            span,
                        );
                        AnalyzerType::Unknown
                    }
                }
            }
        }
    }

    fn check_numeric_pair(
        &mut self,
        op: BinOp,
        left_ty: AnalyzerType,
        right_ty: AnalyzerType,
        span: Span,
    ) -> AnalyzerType {
        if let (
            AnalyzerType::Int {
                signed: ls,
                bits: lb,
            },
            AnalyzerType::Int {
                signed: rs,
                bits: rb,
            },
        ) = (&left_ty, &right_ty)
        {
            if ls == rs && lb == rb {
                return left_ty;
            }
            self.push_error(
                format!("integer widths/signs mismatch for {:?}: left={left_ty:?}, right={right_ty:?}", op),
                span,
            );
            return AnalyzerType::Unknown;
        }

        if let (AnalyzerType::Float { bits: lb }, AnalyzerType::Float { bits: rb }) = (&left_ty, &right_ty) {
            if lb == rb {
                return left_ty;
            }
            self.push_error(
                format!("float widths mismatch for {:?}: left={left_ty:?}, right={right_ty:?}", op),
                span,
            );
            return AnalyzerType::Unknown;
        }

        self.push_error(
            format!("invalid operands for {:?}: left={left_ty:?}, right={right_ty:?}", op),
            span,
        );
        AnalyzerType::Unknown
    }

    fn check_equality_pair(
        &mut self,
        op: BinOp,
        left_ty: AnalyzerType,
        right_ty: AnalyzerType,
        span: Span,
    ) -> AnalyzerType {
        if left_ty == right_ty {
            AnalyzerType::Bool
        } else {
            self.push_error(
                format!("invalid operands for {:?}: left={left_ty:?}, right={right_ty:?}", op),
                span,
            );
            AnalyzerType::Unknown
        }
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
            let expected = if is_variadic_any {
                AnalyzerType::Any
            } else {
                params.get(idx).cloned().unwrap_or(AnalyzerType::Unknown)
            };
            let arg_ty = self.check_expr_expected(*arg, span, Some(&expected));
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

    fn check_int_literal(
        &mut self,
        raw: &str,
        expected: Option<&AnalyzerType>,
        span: Span,
    ) -> AnalyzerType {
        let parsed = match literal_u128(raw) {
            Ok(value) => value,
            Err(_) => {
                self.push_error(format!("invalid integer literal '{raw}'"), span);
                return AnalyzerType::Unknown;
            }
        };

        if let Some(AnalyzerType::Int { signed, bits }) = expected {
            if integer_fits(parsed, *signed, *bits) {
                return AnalyzerType::Int {
                    signed: *signed,
                    bits: *bits,
                };
            }
            self.push_error(
                format!("integer literal '{raw}' out of range for {}{}", if *signed { "i" } else { "u" }, bits),
                span,
            );
            return AnalyzerType::Unknown;
        }

        if integer_fits(parsed, true, 32) {
            AnalyzerType::Int {
                signed: true,
                bits: 32,
            }
        } else if integer_fits(parsed, true, 64) {
            AnalyzerType::Int {
                signed: true,
                bits: 64,
            }
        } else if integer_fits(parsed, true, 128) {
            AnalyzerType::Int {
                signed: true,
                bits: 128,
            }
        } else {
            self.push_error(format!("integer literal '{raw}' out of supported range"), span);
            AnalyzerType::Unknown
        }
    }

    fn check_float_literal(
        &mut self,
        raw: &str,
        expected: Option<&AnalyzerType>,
        span: Span,
    ) -> AnalyzerType {
        let parsed = match literal_f64(raw) {
            Ok(value) => value,
            Err(_) => {
                self.push_error(format!("invalid float literal '{raw}'"), span);
                return AnalyzerType::Unknown;
            }
        };

        if let Some(AnalyzerType::Float { bits }) = expected {
            if float_fits(parsed, *bits) {
                return AnalyzerType::Float { bits: *bits };
            }
            self.push_error(format!("float literal '{raw}' out of range for f{bits}"), span);
            return AnalyzerType::Unknown;
        }

        AnalyzerType::Float { bits: 64 }
    }

    fn push_error(&mut self, message: impl Into<String>, span: Span) {
        self.diagnostics
            .push(Diagnostic::new(message, span, Severity::Error));
    }
}

fn integer_fits(value: u128, signed: bool, bits: u16) -> bool {
    if bits == 0 || bits > 128 {
        return false;
    }
    if signed {
        if bits == 128 {
            return value <= i128::MAX as u128;
        }
        value <= ((1u128 << (bits - 1)) - 1)
    } else if bits == 128 {
        true
    } else {
        value <= ((1u128 << bits) - 1)
    }
}

fn float_fits(value: f64, bits: u16) -> bool {
    match bits {
        32 => value.is_finite() && value >= -(f32::MAX as f64) && value <= f32::MAX as f64,
        64 => value.is_finite(),
        _ => false,
    }
}

fn is_assignable(expected: &AnalyzerType, actual: &AnalyzerType) -> bool {
    expected == actual || matches!(expected, AnalyzerType::Unknown | AnalyzerType::Any) || matches!(actual, AnalyzerType::Unknown)
}

fn is_truthy_falsy_compatible(ty: &AnalyzerType) -> bool {
    matches!(
        ty,
        AnalyzerType::Bool
            | AnalyzerType::Int { .. }
            | AnalyzerType::Float { .. }
            | AnalyzerType::Str
            | AnalyzerType::Char
    )
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
        let (hir, mut symbols, _) = lower(&ast);
        let (_model, diagnostics) = analyze(&hir, &mut symbols);
        assert!(diagnostics.has_errors());
    }

    #[test]
    fn validates_builtin_call_args() {
        let src = "print(1, 2)";
        let lex_output = lex(FileId::from_u32(21), src);
        let (ast, _) = parse(FileId::from_u32(21), src.len() as u32, lex_output.tokens);
        let (hir, mut symbols, _) = lower(&ast);
        let (_model, diagnostics) = analyze(&hir, &mut symbols);
        assert!(!diagnostics.has_errors());
    }

    #[test]
    fn enforces_unsigned_integer_range() {
        let src = "x: u8 = 300";
        let lex_output = lex(FileId::from_u32(22), src);
        let (ast, _) = parse(FileId::from_u32(22), src.len() as u32, lex_output.tokens);
        let (hir, mut symbols, _) = lower(&ast);
        let (_model, diagnostics) = analyze(&hir, &mut symbols);
        assert!(diagnostics.has_errors());
    }

    #[test]
    fn enforces_i1_signed_range() {
        let src = "x: i1 = 1";
        let lex_output = lex(FileId::from_u32(23), src);
        let (ast, _) = parse(FileId::from_u32(23), src.len() as u32, lex_output.tokens);
        let (hir, mut symbols, _) = lower(&ast);
        let (_model, diagnostics) = analyze(&hir, &mut symbols);
        assert!(diagnostics.has_errors());
    }

    #[test]
    fn accepts_u1_upper_bound() {
        let src = "x: u1 = 1";
        let lex_output = lex(FileId::from_u32(24), src);
        let (ast, _) = parse(FileId::from_u32(24), src.len() as u32, lex_output.tokens);
        let (hir, mut symbols, _) = lower(&ast);
        let (_model, diagnostics) = analyze(&hir, &mut symbols);
        assert!(!diagnostics.has_errors());
    }

    #[test]
    fn infers_type_for_colon_equals_binding() {
        let src = "x := 1; y: i32 = x";
        let lex_output = lex(FileId::from_u32(25), src);
        let (ast, _) = parse(FileId::from_u32(25), src.len() as u32, lex_output.tokens);
        let (hir, mut symbols, _) = lower(&ast);
        let (_model, diagnostics) = analyze(&hir, &mut symbols);
        assert!(!diagnostics.has_errors());
    }

    #[test]
    fn supports_char_type_and_literal() {
        let src = "c: char = 'x'";
        let lex_output = lex(FileId::from_u32(26), src);
        let (ast, _) = parse(FileId::from_u32(26), src.len() as u32, lex_output.tokens);
        let (hir, mut symbols, _) = lower(&ast);
        let (_model, diagnostics) = analyze(&hir, &mut symbols);
        assert!(!diagnostics.has_errors());
    }

    #[test]
    fn rejects_assignment_to_const_inside_block() {
        let src = "pi:: f32 = 3.14; { pi = 1.0 }";
        let lex_output = lex(FileId::from_u32(27), src);
        let (ast, _) = parse(FileId::from_u32(27), src.len() as u32, lex_output.tokens);
        let (hir, mut symbols, _) = lower(&ast);
        let (_model, diagnostics) = analyze(&hir, &mut symbols);
        assert!(diagnostics.has_errors());
        assert!(diagnostics.iter().any(|d| d.message.contains("cannot assign to constant")));
    }

    #[test]
    fn accepts_boolean_logical_ops() {
        let src = "a: bool = true; b: bool = false; c: bool = a && !b || a";
        let lex_output = lex(FileId::from_u32(28), src);
        let (ast, _) = parse(FileId::from_u32(28), src.len() as u32, lex_output.tokens);
        let (hir, mut symbols, _) = lower(&ast);
        let (_model, diagnostics) = analyze(&hir, &mut symbols);
        assert!(!diagnostics.has_errors());
    }

    #[test]
    fn rejects_logical_ops_on_integers() {
        let src = "x: i32 = 1; y: i32 = 2; z := x && y";
        let lex_output = lex(FileId::from_u32(29), src);
        let (ast, _) = parse(FileId::from_u32(29), src.len() as u32, lex_output.tokens);
        let (hir, mut symbols, _) = lower(&ast);
        let (_model, diagnostics) = analyze(&hir, &mut symbols);
        assert!(diagnostics.has_errors());
        assert!(diagnostics.iter().any(|d| d.message.contains("logical operator")));
    }

    #[test]
    fn accepts_comparison_and_equality_ops() {
        let src = "a: i32 = 2; b: i32 = 3; lt: bool = a < b; eq: bool = a == b";
        let lex_output = lex(FileId::from_u32(30), src);
        let (ast, _) = parse(FileId::from_u32(30), src.len() as u32, lex_output.tokens);
        let (hir, mut symbols, _) = lower(&ast);
        let (_model, diagnostics) = analyze(&hir, &mut symbols);
        assert!(!diagnostics.has_errors());
    }

    #[test]
    fn accepts_bitwise_and_shift_integer_ops() {
        let src = "a: u32 = 0xF0; b: u32 = 0x0F; c: u32 = (a & b) | (a ^ b); d: u32 = c << 2; e: u32 = d >> 1";
        let lex_output = lex(FileId::from_u32(31), src);
        let (ast, _) = parse(FileId::from_u32(31), src.len() as u32, lex_output.tokens);
        let (hir, mut symbols, _) = lower(&ast);
        let (_model, diagnostics) = analyze(&hir, &mut symbols);
        assert!(!diagnostics.has_errors());
    }

    #[test]
    fn based_literal_range_is_validated() {
        let src = "x: u8 = 0x1FF";
        let lex_output = lex(FileId::from_u32(32), src);
        let (ast, _) = parse(FileId::from_u32(32), src.len() as u32, lex_output.tokens);
        let (hir, mut symbols, _) = lower(&ast);
        let (_model, diagnostics) = analyze(&hir, &mut symbols);
        assert!(diagnostics.has_errors());
        assert!(diagnostics.iter().any(|d| d.message.contains("out of range")));
    }

    #[test]
    fn allows_truthy_numeric_if_condition() {
        let src = "if 1 { x := 1 } else { x := 2 }";
        let lex_output = lex(FileId::from_u32(33), src);
        let (ast, _) = parse(FileId::from_u32(33), src.len() as u32, lex_output.tokens);
        let (hir, mut symbols, _) = lower(&ast);
        let (_model, diagnostics) = analyze(&hir, &mut symbols);
        assert!(!diagnostics.has_errors());
    }

    #[test]
    fn rejects_unit_if_condition() {
        let src = "if print(1) { x := 1 }";
        let lex_output = lex(FileId::from_u32(34), src);
        let (ast, _) = parse(FileId::from_u32(34), src.len() as u32, lex_output.tokens);
        let (hir, mut symbols, _) = lower(&ast);
        let (_model, diagnostics) = analyze(&hir, &mut symbols);
        assert!(diagnostics.has_errors());
        assert!(diagnostics
            .iter()
            .any(|d| d.message.contains("if condition is not truthy/falsy-compatible")));
    }
}
