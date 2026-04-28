//! Lower [`Hir`] + [`crate::analyzer::SemanticModel`] into [`Chunk`] (no AST dependency).

use foundation::{
    diagnostics::{Diagnostic, Diagnostics, Severity},
    span::Span,
};

use crate::{
    analyzer::SemanticModel,
    hir::{BinOp, Hir, HirExpr, HirId, HirStmt},
};

use super::{
    bytecode::Op,
    chunk::{Chunk, ChunkBuilder},
};

pub fn compile_program(hir: &Hir, model: &SemanticModel) -> (Chunk, Diagnostics) {
    let mut builder = ChunkBuilder::new();
    let mut diagnostics = Diagnostics::new();

    for stmt in &hir.stmts {
        emit_stmt(hir, model, stmt, &mut builder, &mut diagnostics);
    }

    let ret_span = hir
        .stmts
        .last()
        .map(stmt_primary_span)
        .unwrap_or_else(|| Span::new_unchecked(hir.file_id, 0, 0));
    builder.emit(Op::Return, ret_span);

    (builder.finish(), diagnostics)
}

fn stmt_primary_span(stmt: &HirStmt) -> Span {
    match stmt {
        HirStmt::Let { span, .. } | HirStmt::Expr { span, .. } | HirStmt::Invalid { span } => *span,
    }
}

fn expr_span(hir: &Hir, id: HirId) -> Span {
    hir.expr_spans
        .get(&id)
        .copied()
        .unwrap_or_else(|| Span::new_unchecked(hir.file_id, 0, 0))
}

fn emit_stmt(
    hir: &Hir,
    model: &SemanticModel,
    stmt: &HirStmt,
    b: &mut ChunkBuilder,
    diagnostics: &mut Diagnostics,
) {
    match stmt {
        HirStmt::Let {
            symbol, value, span, ..
        } => {
            emit_expr(hir, model, *value, b, diagnostics);
            b.emit(Op::Store(*symbol), *span);
        }
        HirStmt::Expr { expr, span } => {
            emit_expr(hir, model, *expr, b, diagnostics);
            b.emit(Op::Pop, *span);
        }
        HirStmt::Invalid { span } => {
            diagnostics.push(Diagnostic::new("invalid statement skipped in bytecode", *span, Severity::Error));
        }
    }
}

fn emit_expr(
    hir: &Hir,
    model: &SemanticModel,
    id: HirId,
    b: &mut ChunkBuilder,
    diagnostics: &mut Diagnostics,
) {
    let span = expr_span(hir, id);

    let Some(expr) = hir.exprs.get(id) else {
        diagnostics.push(Diagnostic::new("missing hir expression", span, Severity::Error));
        return;
    };

    match expr {
        HirExpr::Int(raw) => match parse_int_literal(raw, span, diagnostics) {
            Some(v) => b.emit(Op::ConstInt(v), span),
            None => {}
        },
        HirExpr::Float(raw) => match raw.parse::<f64>() {
            Ok(v) => b.emit(Op::ConstFloat(v), span),
            Err(_) => {
                diagnostics.push(Diagnostic::new(
                    format!("invalid float literal `{raw}` in bytecode"),
                    span,
                    Severity::Error,
                ));
            }
        },
        HirExpr::Bool(v) => b.emit(Op::ConstBool(*v), span),
        HirExpr::Str(s) => b.emit(Op::ConstStr(s.clone()), span),
        HirExpr::Char(c) => b.emit(Op::ConstChar(*c), span),
        HirExpr::Var(sym) => b.emit(Op::Load(*sym), span),
        HirExpr::Binary {
            op: binop,
            lhs,
            rhs,
        } => {
            emit_expr(hir, model, *lhs, b, diagnostics);
            emit_expr(hir, model, *rhs, b, diagnostics);
            let span = expr_span(hir, id);
            match binop {
                BinOp::Add => b.emit(Op::Add, span),
                BinOp::Subtract => b.emit(Op::Sub, span),
                BinOp::Multiply => b.emit(Op::Mul, span),
                BinOp::Divide => b.emit(Op::Div, span),
            }
        }
        HirExpr::Call { callee, args } => {
            for a in args {
                emit_expr(hir, model, *a, b, diagnostics);
            }
            match u8::try_from(args.len()) {
                Ok(argc) => b.emit(Op::Call(*callee, argc), span),
                Err(_) => {
                    diagnostics.push(Diagnostic::new(
                        "too many arguments for call (u8 overflow)",
                        span,
                        Severity::Error,
                    ));
                }
            }
        }
        HirExpr::Invalid => {
            diagnostics.push(Diagnostic::new("invalid expression in bytecode", span, Severity::Error));
        }
    }
}

fn parse_int_literal(raw: &str, span: Span, diagnostics: &mut Diagnostics) -> Option<i64> {
    match raw.parse::<i64>() {
        Ok(v) => Some(v),
        Err(_) => {
            diagnostics.push(Diagnostic::new(
                format!("invalid integer literal `{raw}` in bytecode"),
                span,
                Severity::Error,
            ));
            None
        }
    }
}
