//! Lower [`Hir`] + [`crate::analyzer::SemanticModel`] into [`Chunk`] (no AST dependency).

use foundation::{
    diagnostics::{Diagnostic, Diagnostics, Severity},
    span::Span,
};

use crate::{
    analyzer::{SemanticModel, Type},
    hir::{
        BinOp, Hir, HirExpr, HirId, HirStmt, IncDecOp as HirIncDecOp, IncDecPosition as HirIncDecPosition, SymbolId,
        UnaryOp as HirUnaryOp,
    },
    integer_lit::{bytecode_int_from_checked_literal, literal_f64, literal_u128, IntConst},
};

use super::{
    bytecode::Op,
    chunk::{Chunk, ChunkBuilder, FunctionChunk},
};

pub fn compile_program(hir: &Hir, model: &SemanticModel) -> (Chunk, Diagnostics) {
    let mut builder = ChunkBuilder::new();
    let mut diagnostics = Diagnostics::new();
    let mut loop_stack = Vec::new();
    let mut scope_depth = 0usize;
    let mut method_table: std::collections::HashMap<(SymbolId, String, bool), SymbolId> =
        std::collections::HashMap::new();
    collect_method_table(hir, &mut method_table);

    for stmt in &hir.stmts {
        emit_stmt(
            hir,
            model,
            stmt,
            &mut builder,
            &mut diagnostics,
            &mut loop_stack,
            &mut scope_depth,
            &method_table,
        );
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
        HirStmt::Let { span, .. }
        | HirStmt::FnDecl { span, .. }
        | HirStmt::StructDecl { span, .. }
        | HirStmt::TraitDecl { span, .. }
        | HirStmt::ImplBlock { span, .. }
        | HirStmt::TupleDestructure { span, .. }
        | HirStmt::Assign { span, .. }
        | HirStmt::Expr { span, .. }
        | HirStmt::Block { span, .. }
        | HirStmt::If { span, .. }
        | HirStmt::While { span, .. }
        | HirStmt::For { span, .. }
        | HirStmt::Break { span, .. }
        | HirStmt::Continue { span, .. }
        | HirStmt::Return { span, .. }
        | HirStmt::Invalid { span } => *span,
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
    loop_stack: &mut Vec<LoopContext>,
    scope_depth: &mut usize,
    method_table: &std::collections::HashMap<(SymbolId, String, bool), SymbolId>,
) {
    match stmt {
        HirStmt::Let {
            symbol, value, span, ..
        } => {
            emit_expr(hir, model, *value, b, diagnostics, method_table);
            b.emit(Op::Bind(*symbol), *span);
        }
        HirStmt::FnDecl {
            symbol,
            params,
            body,
            span,
            ..
        } => {
            let function_chunk =
                compile_function_chunk(hir, model, params.clone(), body, diagnostics, method_table);
            b.define_function(*symbol, function_chunk);
            b.emit(Op::MakeClosure(*symbol), *span);
            b.emit(Op::Bind(*symbol), *span);
        }
        HirStmt::StructDecl { .. } | HirStmt::TraitDecl { .. } => {}
        HirStmt::ImplBlock { methods, .. } => {
            for method in methods {
                emit_stmt(
                    hir,
                    model,
                    method,
                    b,
                    diagnostics,
                    loop_stack,
                    scope_depth,
                    method_table,
                );
            }
        }
        HirStmt::TupleDestructure {
            names,
            value,
            span,
            ..
        } => {
            emit_expr(hir, model, *value, b, diagnostics, method_table);
            for (idx, sym) in names.iter().enumerate() {
                b.emit(Op::Dup, *span);
                b.emit(Op::TupleGet(idx), *span);
                b.emit(Op::Bind(*sym), *span);
            }
            b.emit(Op::Pop, *span);
        }
        HirStmt::Assign {
            symbol, value, span, ..
        } => {
            emit_expr(hir, model, *value, b, diagnostics, method_table);
            b.emit(Op::Assign(*symbol), *span);
        }
        HirStmt::Expr { expr, span } => {
            emit_expr(hir, model, *expr, b, diagnostics, method_table);
            b.emit(Op::Pop, *span);
        }
        HirStmt::Block { stmts, span } => {
            b.emit(Op::EnterScope, *span);
            *scope_depth += 1;
            for stmt in stmts {
                emit_stmt(hir, model, stmt, b, diagnostics, loop_stack, scope_depth, method_table);
            }
            b.emit(Op::ExitScope, *span);
            *scope_depth = scope_depth.saturating_sub(1);
        }
        HirStmt::If {
            condition,
            then_branch,
            else_branch,
            span,
        } => {
            emit_expr(hir, model, *condition, b, diagnostics, method_table);
            let jump_if_false_at = b.emit_placeholder_jump_if_false(*span);
            b.emit(Op::EnterScope, *span);
            *scope_depth += 1;
            for stmt in then_branch {
                emit_stmt(hir, model, stmt, b, diagnostics, loop_stack, scope_depth, method_table);
            }
            b.emit(Op::ExitScope, *span);
            *scope_depth = scope_depth.saturating_sub(1);
            if let Some(else_stmts) = else_branch {
                let jump_end_at = b.emit_placeholder_jump(*span);
                let else_start = b.len();
                if !b.patch_jump_target(jump_if_false_at, else_start) {
                    diagnostics.push(Diagnostic::new("failed to patch conditional jump", *span, Severity::Error));
                }
                b.emit(Op::EnterScope, *span);
                *scope_depth += 1;
                for stmt in else_stmts {
                    emit_stmt(hir, model, stmt, b, diagnostics, loop_stack, scope_depth, method_table);
                }
                b.emit(Op::ExitScope, *span);
                *scope_depth = scope_depth.saturating_sub(1);
                let end = b.len();
                if !b.patch_jump_target(jump_end_at, end) {
                    diagnostics.push(Diagnostic::new("failed to patch end jump", *span, Severity::Error));
                }
            } else {
                let end = b.len();
                if !b.patch_jump_target(jump_if_false_at, end) {
                    diagnostics.push(Diagnostic::new("failed to patch conditional jump", *span, Severity::Error));
                }
            }
        }
        HirStmt::While {
            condition,
            body,
            span,
        } => {
            let cond_target = b.len();
            emit_expr(hir, model, *condition, b, diagnostics, method_table);
            let jump_out_at = b.emit_placeholder_jump_if_false(*span);

            loop_stack.push(LoopContext {
                continue_target: Some(cond_target),
                break_sites: Vec::new(),
                continue_sites: Vec::new(),
                scope_depth_at_loop: *scope_depth,
            });

            b.emit(Op::EnterScope, *span);
            *scope_depth += 1;
            for stmt in body {
                emit_stmt(hir, model, stmt, b, diagnostics, loop_stack, scope_depth, method_table);
            }
            b.emit(Op::ExitScope, *span);
            *scope_depth = scope_depth.saturating_sub(1);

            b.emit(Op::Jump(cond_target), *span);
            let loop_end = b.len();
            if !b.patch_jump_target(jump_out_at, loop_end) {
                diagnostics.push(Diagnostic::new("failed to patch while exit jump", *span, Severity::Error));
            }
            if let Some(ctx) = loop_stack.pop() {
                for site in ctx.break_sites {
                    if !b.patch_jump_target(site, loop_end) {
                        diagnostics.push(Diagnostic::new("failed to patch break jump", *span, Severity::Error));
                    }
                }
                if let Some(continue_target) = ctx.continue_target {
                    for site in ctx.continue_sites {
                        if !b.patch_jump_target(site, continue_target) {
                            diagnostics.push(Diagnostic::new("failed to patch continue jump", *span, Severity::Error));
                        }
                    }
                }
            } else {
                diagnostics.push(Diagnostic::new("internal loop stack underflow in emitter", *span, Severity::Error));
            }
        }
        HirStmt::For {
            init,
            condition,
            step,
            body,
            span,
        } => {
            b.emit(Op::EnterScope, *span);
            *scope_depth += 1;
            if let Some(init_stmt) = init {
                emit_stmt(hir, model, init_stmt, b, diagnostics, loop_stack, scope_depth, method_table);
            }

            let cond_target = b.len();
            if let Some(condition_expr) = condition {
                emit_expr(hir, model, *condition_expr, b, diagnostics, method_table);
            } else {
                b.emit(Op::ConstBool(true), *span);
            }
            let jump_out_at = b.emit_placeholder_jump_if_false(*span);

            loop_stack.push(LoopContext {
                continue_target: None,
                break_sites: Vec::new(),
                continue_sites: Vec::new(),
                scope_depth_at_loop: *scope_depth,
            });

            b.emit(Op::EnterScope, *span);
            *scope_depth += 1;
            for stmt in body {
                emit_stmt(hir, model, stmt, b, diagnostics, loop_stack, scope_depth, method_table);
            }
            b.emit(Op::ExitScope, *span);
            *scope_depth = scope_depth.saturating_sub(1);

            let continue_target = if let Some(step_expr) = step {
                let target = b.len();
                emit_expr(hir, model, *step_expr, b, diagnostics, method_table);
                b.emit(Op::Pop, *span);
                target
            } else {
                cond_target
            };
            b.emit(Op::Jump(cond_target), *span);
            let loop_end = b.len();

            if !b.patch_jump_target(jump_out_at, loop_end) {
                diagnostics.push(Diagnostic::new("failed to patch for exit jump", *span, Severity::Error));
            }
            if let Some(ctx) = loop_stack.pop() {
                for site in ctx.break_sites {
                    if !b.patch_jump_target(site, loop_end) {
                        diagnostics.push(Diagnostic::new("failed to patch break jump", *span, Severity::Error));
                    }
                }
                for site in ctx.continue_sites {
                    if !b.patch_jump_target(site, continue_target) {
                        diagnostics.push(Diagnostic::new("failed to patch continue jump", *span, Severity::Error));
                    }
                }
            } else {
                diagnostics.push(Diagnostic::new("internal loop stack underflow in emitter", *span, Severity::Error));
            }

            b.emit(Op::ExitScope, *span);
            *scope_depth = scope_depth.saturating_sub(1);
        }
        HirStmt::Break { span } => {
            let Some(loop_ctx) = loop_stack.last_mut() else {
                diagnostics.push(Diagnostic::new("break used outside of loop", *span, Severity::Error));
                return;
            };
            emit_scope_unwind_for_loop_exit(b, *span, *scope_depth, loop_ctx.scope_depth_at_loop);
            let break_jump = b.emit_placeholder_jump(*span);
            loop_ctx.break_sites.push(break_jump);
        }
        HirStmt::Continue { span } => {
            let Some(loop_ctx) = loop_stack.last() else {
                diagnostics.push(Diagnostic::new("continue used outside of loop", *span, Severity::Error));
                return;
            };
            emit_scope_unwind_for_loop_exit(b, *span, *scope_depth, loop_ctx.scope_depth_at_loop);
            let continue_jump = b.emit_placeholder_jump(*span);
            if let Some(loop_ctx_mut) = loop_stack.last_mut() {
                loop_ctx_mut.continue_sites.push(continue_jump);
            }
        }
        HirStmt::Return { values, span } => {
            if values.is_empty() {
                b.emit(Op::ConstUnit, *span);
            } else if values.len() == 1 {
                emit_expr(hir, model, values[0], b, diagnostics, method_table);
            } else {
                for expr in values {
                    emit_expr(hir, model, *expr, b, diagnostics, method_table);
                }
                match u8::try_from(values.len()) {
                    Ok(count) => b.emit(Op::MakeTuple(count), *span),
                    Err(_) => diagnostics.push(Diagnostic::new(
                        "too many values in return tuple",
                        *span,
                        Severity::Error,
                    )),
                }
            }
            b.emit(Op::Return, *span);
        }
        HirStmt::Invalid { span } => {
            diagnostics.push(Diagnostic::new("invalid statement skipped in bytecode", *span, Severity::Error));
        }
    }
}

#[derive(Debug, Clone)]
struct LoopContext {
    continue_target: Option<usize>,
    break_sites: Vec<usize>,
    continue_sites: Vec<usize>,
    scope_depth_at_loop: usize,
}

fn emit_scope_unwind_for_loop_exit(
    b: &mut ChunkBuilder,
    span: Span,
    current_scope_depth: usize,
    target_scope_depth: usize,
) {
    let unwind_count = current_scope_depth.saturating_sub(target_scope_depth);
    for _ in 0..unwind_count {
        b.emit(Op::ExitScope, span);
    }
}

fn emit_expr(
    hir: &Hir,
    model: &SemanticModel,
    id: HirId,
    b: &mut ChunkBuilder,
    diagnostics: &mut Diagnostics,
    method_table: &std::collections::HashMap<(SymbolId, String, bool), SymbolId>,
) {
    let span = expr_span(hir, id);

    let Some(expr) = hir.exprs.get(id) else {
        diagnostics.push(Diagnostic::new("missing hir expression", span, Severity::Error));
        return;
    };

    match expr {
        HirExpr::Int(raw) => {
            let hir_ty = model
                .types
                .get(&id)
                .cloned()
                .unwrap_or(Type::Unknown);

            match emit_int_const(raw, &hir_ty) {
                Ok(IntConst::Signed(v)) => b.emit(Op::ConstI128(v), span),
                Ok(IntConst::Unsigned(v)) => b.emit(Op::ConstU128(v), span),
                Err(msg) => {
                    diagnostics.push(Diagnostic::new(
                        format!("integer literal `{raw}`: {msg}"),
                        span,
                        Severity::Error,
                    ));
                }
            }
        }
        HirExpr::Float(raw) => match literal_f64(raw) {
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
        HirExpr::Null => b.emit(Op::ConstNull, span),
        HirExpr::Str(s) => b.emit(Op::ConstStr(s.clone()), span),
        HirExpr::Char(c) => b.emit(Op::ConstChar(*c), span),
        HirExpr::Var(sym) => b.emit(Op::Load(*sym), span),
        HirExpr::Unary {
            op: HirUnaryOp::Neg,
            operand,
        } => {
            emit_expr(hir, model, *operand, b, diagnostics, method_table);
            b.emit(Op::Neg, expr_span(hir, id));
        }
        HirExpr::Unary {
            op: HirUnaryOp::Not,
            operand,
        } => {
            emit_expr(hir, model, *operand, b, diagnostics, method_table);
            b.emit(Op::Not, expr_span(hir, id));
        }
        HirExpr::Unary {
            op: HirUnaryOp::BitNot,
            operand,
        } => {
            emit_expr(hir, model, *operand, b, diagnostics, method_table);
            b.emit(Op::BitNot, expr_span(hir, id));
        }
        HirExpr::Binary {
            op: binop,
            lhs,
            rhs,
        } => {
            emit_expr(hir, model, *lhs, b, diagnostics, method_table);
            emit_expr(hir, model, *rhs, b, diagnostics, method_table);
            let span_merge = expr_span(hir, id);
            match binop {
                BinOp::Add => b.emit(Op::Add, span_merge),
                BinOp::Subtract => b.emit(Op::Sub, span_merge),
                BinOp::Multiply => b.emit(Op::Mul, span_merge),
                BinOp::Divide => b.emit(Op::Div, span_merge),
                BinOp::Modulo => b.emit(Op::Mod, span_merge),
                BinOp::Power => b.emit(Op::Pow, span_merge),
                BinOp::Equal => b.emit(Op::Eq, span_merge),
                BinOp::NotEqual => b.emit(Op::Ne, span_merge),
                BinOp::Less => b.emit(Op::Lt, span_merge),
                BinOp::LessEqual => b.emit(Op::Le, span_merge),
                BinOp::Greater => b.emit(Op::Gt, span_merge),
                BinOp::GreaterEqual => b.emit(Op::Ge, span_merge),
                BinOp::LogicalAnd => b.emit(Op::LogicalAnd, span_merge),
                BinOp::LogicalOr => b.emit(Op::LogicalOr, span_merge),
                BinOp::BitAnd => b.emit(Op::BitAnd, span_merge),
                BinOp::BitOr => b.emit(Op::BitOr, span_merge),
                BinOp::BitXor => b.emit(Op::BitXor, span_merge),
                BinOp::ShiftLeft => b.emit(Op::Shl, span_merge),
                BinOp::ShiftRight => b.emit(Op::Shr, span_merge),
            }
        }
        HirExpr::Call { callee, args } => {
            emit_expr(hir, model, *callee, b, diagnostics, method_table);
            for a in args {
                emit_expr(hir, model, *a, b, diagnostics, method_table);
            }
            match u8::try_from(args.len()) {
                Ok(argc) => b.emit(Op::CallValue(argc), span),
                Err(_) => {
                    diagnostics.push(Diagnostic::new(
                        "too many arguments for call (u8 overflow)",
                        span,
                        Severity::Error,
                    ));
                }
            }
        }
        HirExpr::StructLiteral { type_name, fields } => {
            for (_, value) in fields {
                emit_expr(hir, model, *value, b, diagnostics, method_table);
            }
            b.emit(
                Op::MakeStruct(
                    type_name.clone(),
                    fields.iter().map(|(n, _)| n.clone()).collect(),
                ),
                span,
            );
        }
        HirExpr::FieldAccess { base, field } => {
            emit_expr(hir, model, *base, b, diagnostics, method_table);
            b.emit(Op::StructGet(field.clone()), span);
        }
        HirExpr::MethodCall {
            receiver,
            method,
            args,
        } => {
            let recv_ty = model.types.get(receiver).cloned().unwrap_or(Type::Unknown);
            let Type::Struct(struct_id) = recv_ty else {
                diagnostics.push(Diagnostic::new("method call receiver is not struct", span, Severity::Error));
                return;
            };
            let Some(method_symbol) = method_table.get(&(struct_id, method.clone(), true)).copied() else {
                diagnostics.push(Diagnostic::new(format!("unknown method '{}'", method), span, Severity::Error));
                return;
            };
            b.emit(Op::Load(method_symbol), span);
            emit_expr(hir, model, *receiver, b, diagnostics, method_table);
            for arg in args {
                emit_expr(hir, model, *arg, b, diagnostics, method_table);
            }
            match u8::try_from(args.len() + 1) {
                Ok(argc) => b.emit(Op::CallValue(argc), span),
                Err(_) => diagnostics.push(Diagnostic::new("too many arguments for method call", span, Severity::Error)),
            }
        }
        HirExpr::StaticMethodCall {
            type_name,
            method,
            args,
        } => {
            let struct_id = match resolve_struct_symbol_id_from_type_name(hir, model, type_name) {
                Some(id) => id,
                None => {
                    diagnostics.push(Diagnostic::new(
                        format!("unknown struct type '{}'", type_name),
                        span,
                        Severity::Error,
                    ));
                    return;
                }
            };
            let Some(method_symbol) = method_table.get(&(struct_id, method.clone(), false)).copied() else {
                diagnostics.push(Diagnostic::new(
                    format!("unknown static method '{}::{}'", type_name, method),
                    span,
                    Severity::Error,
                ));
                return;
            };
            b.emit(Op::Load(method_symbol), span);
            for arg in args {
                emit_expr(hir, model, *arg, b, diagnostics, method_table);
            }
            match u8::try_from(args.len()) {
                Ok(argc) => b.emit(Op::CallValue(argc), span),
                Err(_) => diagnostics.push(Diagnostic::new(
                    "too many arguments for static method call",
                    span,
                    Severity::Error,
                )),
            }
        }
        HirExpr::Tuple(items) => {
            for item in items {
                emit_expr(hir, model, *item, b, diagnostics, method_table);
            }
            match u8::try_from(items.len()) {
                Ok(count) => b.emit(Op::MakeTuple(count), span),
                Err(_) => diagnostics.push(Diagnostic::new("tuple literal too large", span, Severity::Error)),
            }
        }
        HirExpr::TupleAccess { tuple, index } => {
            emit_expr(hir, model, *tuple, b, diagnostics, method_table);
            b.emit(Op::TupleGet(*index), span);
        }
        HirExpr::IncDec {
            symbol,
            op,
            position,
        } => {
            let ty = model.types.get(&id).cloned().unwrap_or(Type::Unknown);
            match position {
                HirIncDecPosition::Prefix => {
                    b.emit(Op::Load(*symbol), span);
                    emit_numeric_one_const(b, span, &ty, diagnostics);
                    match op {
                        HirIncDecOp::Increment => b.emit(Op::Add, span),
                        HirIncDecOp::Decrement => b.emit(Op::Sub, span),
                    }
                    b.emit(Op::Assign(*symbol), span);
                    b.emit(Op::Load(*symbol), span);
                }
                HirIncDecPosition::Postfix => {
                    b.emit(Op::Load(*symbol), span);
                    b.emit(Op::Load(*symbol), span);
                    emit_numeric_one_const(b, span, &ty, diagnostics);
                    match op {
                        HirIncDecOp::Increment => b.emit(Op::Add, span),
                        HirIncDecOp::Decrement => b.emit(Op::Sub, span),
                    }
                    b.emit(Op::Assign(*symbol), span);
                }
            }
        }
        HirExpr::Propagate { expr } => {
            emit_expr(hir, model, *expr, b, diagnostics, method_table);
            b.emit(Op::Dup, span);
            b.emit(Op::TupleGet(1), span);
            b.emit(Op::ConstNull, span);
            b.emit(Op::Ne, span);
            let continue_at = b.emit_placeholder_jump_if_false(span);
            b.emit(Op::TupleGet(1), span);
            b.emit(Op::ConstStr("propagated by ?".to_string()), span);
            b.emit(Op::WrapErr, span);
            b.emit(Op::ConstNull, span);
            b.emit(Op::Swap, span);
            b.emit(Op::MakeTuple(2), span);
            b.emit(Op::Return, span);
            let ok_target = b.len();
            if !b.patch_jump_target(continue_at, ok_target) {
                diagnostics.push(Diagnostic::new("failed to patch '?' continue jump", span, Severity::Error));
            }
            b.emit(Op::TupleGet(0), span);
        }
        HirExpr::TryCatch {
            try_expr,
            err_symbol,
            catch_stmts,
            catch_value,
        } => {
            let panic_handler_jump = b.emit_placeholder_try_start(span);
            emit_expr(hir, model, *try_expr, b, diagnostics, method_table);
            b.emit(Op::TryEnd, span);
            let skip_panic_handler = b.emit_placeholder_jump(span);
            let panic_handler_label = b.len();
            b.emit(Op::ConstNull, span);
            b.emit(Op::Swap, span);
            b.emit(Op::MakeTuple(2), span);
            if !b.patch_jump_target(panic_handler_jump, panic_handler_label) {
                diagnostics.push(Diagnostic::new(
                    "failed to patch try panic handler jump",
                    span,
                    Severity::Error,
                ));
            }
            let post_panic_label = b.len();
            if !b.patch_jump_target(skip_panic_handler, post_panic_label) {
                diagnostics.push(Diagnostic::new(
                    "failed to patch try panic skip jump",
                    span,
                    Severity::Error,
                ));
            }

            b.emit(Op::Dup, span);
            b.emit(Op::TupleGet(1), span);
            b.emit(Op::ConstNull, span);
            b.emit(Op::Ne, span);
            let success_jump = b.emit_placeholder_jump_if_false(span);
            b.emit(Op::Dup, span);
            b.emit(Op::TupleGet(1), span);
            b.emit(Op::EnterScope, span);
            b.emit(Op::Bind(*err_symbol), span);
            b.emit(Op::Pop, span);
            let mut catch_loop_stack = Vec::new();
            let mut catch_scope_depth = 1usize;
            for stmt in catch_stmts {
                emit_stmt(
                    hir,
                    model,
                    stmt,
                    b,
                    diagnostics,
                    &mut catch_loop_stack,
                    &mut catch_scope_depth,
                    method_table,
                );
            }
            emit_expr(hir, model, *catch_value, b, diagnostics, method_table);
            b.emit(Op::ExitScope, span);
            let end_jump = b.emit_placeholder_jump(span);
            let success_label = b.len();
            if !b.patch_jump_target(success_jump, success_label) {
                diagnostics.push(Diagnostic::new("failed to patch try success jump", span, Severity::Error));
            }
            b.emit(Op::TupleGet(0), span);
            let end_label = b.len();
            if !b.patch_jump_target(end_jump, end_label) {
                diagnostics.push(Diagnostic::new("failed to patch try end jump", span, Severity::Error));
            }
        }
        HirExpr::Invalid => {
            diagnostics.push(Diagnostic::new("invalid expression in bytecode", span, Severity::Error));
        }
    }
}

fn compile_function_chunk(
    hir: &Hir,
    model: &SemanticModel,
    params: Vec<crate::hir::SymbolId>,
    body: &[HirStmt],
    diagnostics: &mut Diagnostics,
    method_table: &std::collections::HashMap<(SymbolId, String, bool), SymbolId>,
) -> FunctionChunk {
    let mut b = ChunkBuilder::new();
    let mut loop_stack = Vec::new();
    let mut scope_depth = 0usize;
    for stmt in body {
        emit_stmt(
            hir,
            model,
            stmt,
            &mut b,
            diagnostics,
            &mut loop_stack,
            &mut scope_depth,
            method_table,
        );
    }
    b.emit(
        Op::ConstUnit,
        Span::new_unchecked(hir.file_id, 0, 0),
    );
    b.emit(Op::Return, Span::new_unchecked(hir.file_id, 0, 0));
    FunctionChunk {
        params,
        chunk: b.finish(),
    }
}

fn collect_method_table(
    hir: &Hir,
    table: &mut std::collections::HashMap<(SymbolId, String, bool), SymbolId>,
) {
    for stmt in &hir.stmts {
        if let HirStmt::ImplBlock { target, methods, .. } = stmt {
            let Type::Struct(struct_id) = target else {
                continue;
            };
            for method_stmt in methods {
                if let HirStmt::FnDecl {
                    symbol,
                    name,
                    is_instance,
                    ..
                } = method_stmt
                {
                    table.insert((*struct_id, name.clone(), *is_instance), *symbol);
                }
            }
        }
    }
}

fn resolve_struct_symbol_id_from_type_name(hir: &Hir, _model: &SemanticModel, type_name: &str) -> Option<SymbolId> {
    for stmt in &hir.stmts {
        if let HirStmt::StructDecl { symbol, name, .. } = stmt {
            if name == type_name {
                return Some(*symbol);
            }
        }
    }
    None
}

fn emit_numeric_one_const(b: &mut ChunkBuilder, span: Span, ty: &Type, diagnostics: &mut Diagnostics) {
    match ty {
        Type::Int { signed: true, .. } => b.emit(Op::ConstI128(1), span),
        Type::Int { signed: false, .. } => b.emit(Op::ConstU128(1), span),
        Type::Float { .. } => b.emit(Op::ConstFloat(1.0), span),
        _ => diagnostics.push(Diagnostic::new(
            "increment/decrement requires numeric type in emitter",
            span,
            Severity::Error,
        )),
    }
}

/// Integer literals use the semantic type produced by analyze; fallback matches checker inference ladder.
fn emit_int_const(raw: &str, hir_ty: &Type) -> Result<IntConst, &'static str> {
    match hir_ty {
        Type::Unknown => fallback_inference_int(raw),
        ty @ Type::Int { .. } => bytecode_int_from_checked_literal(raw, ty),
        // Defensive fallback if analyzer and HIR ever diverge:
        _ => fallback_inference_int(raw),
    }
}

fn fallback_inference_int(raw: &str) -> Result<IntConst, &'static str> {
    let parsed = literal_u128(raw)?;
    if parsed <= i128::MAX as u128 {
        Ok(IntConst::Signed(parsed as i128))
    } else {
        Ok(IntConst::Unsigned(parsed))
    }
}

#[cfg(test)]
mod tests {
    use foundation::ids::FileId;

    use crate::{analyzer::analyze, lexer::lex, lowering::lower, parser::parse, vm::bytecode::Op};

    use super::compile_program;

    #[test]
    fn emits_scope_ops_for_block_stmt() {
        let src = "{ x := 1; print(x) }";
        let lex_output = lex(FileId::from_u32(31), src);
        let (ast, parser_diagnostics) = parse(FileId::from_u32(31), src.len() as u32, lex_output.tokens);
        assert!(!parser_diagnostics.has_errors());
        let (hir, mut symbols, lowering_diagnostics) = lower(&ast);
        assert!(!lowering_diagnostics.has_errors());
        let (model, analysis_diagnostics) = analyze(&hir, &mut symbols);
        assert!(!analysis_diagnostics.has_errors());

        let (chunk, compile_diagnostics) = compile_program(&hir, &model);
        assert!(!compile_diagnostics.has_errors());
        assert!(chunk.code.iter().any(|op| matches!(op, Op::EnterScope)));
        assert!(chunk.code.iter().any(|op| matches!(op, Op::ExitScope)));
    }

    #[test]
    fn emits_new_operator_ops() {
        let src = "x := ((5 % 2) == 1) && !false; y := ~3; z := 2 ** 3";
        let lex_output = lex(FileId::from_u32(32), src);
        let (ast, parser_diagnostics) = parse(FileId::from_u32(32), src.len() as u32, lex_output.tokens);
        assert!(!parser_diagnostics.has_errors());
        let (hir, mut symbols, lowering_diagnostics) = lower(&ast);
        assert!(!lowering_diagnostics.has_errors());
        let (model, analysis_diagnostics) = analyze(&hir, &mut symbols);
        assert!(!analysis_diagnostics.has_errors());
        let (chunk, compile_diagnostics) = compile_program(&hir, &model);
        assert!(!compile_diagnostics.has_errors());
        assert!(chunk.code.iter().any(|op| matches!(op, Op::Mod)));
        assert!(chunk.code.iter().any(|op| matches!(op, Op::Eq)));
        assert!(chunk.code.iter().any(|op| matches!(op, Op::LogicalAnd)));
        assert!(chunk.code.iter().any(|op| matches!(op, Op::Not)));
        assert!(chunk.code.iter().any(|op| matches!(op, Op::BitNot)));
        assert!(chunk.code.iter().any(|op| matches!(op, Op::Pow)));
    }

    #[test]
    fn emits_if_jumps() {
        let src = "if true { x := 1 } else { x := 2 }";
        let lex_output = lex(FileId::from_u32(33), src);
        let (ast, parser_diagnostics) = parse(FileId::from_u32(33), src.len() as u32, lex_output.tokens);
        assert!(!parser_diagnostics.has_errors());
        let (hir, mut symbols, lowering_diagnostics) = lower(&ast);
        assert!(!lowering_diagnostics.has_errors());
        let (model, analysis_diagnostics) = analyze(&hir, &mut symbols);
        assert!(!analysis_diagnostics.has_errors());
        let (chunk, compile_diagnostics) = compile_program(&hir, &model);
        assert!(!compile_diagnostics.has_errors());
        assert!(chunk.code.iter().any(|op| matches!(op, Op::JumpIfFalse(_))));
        assert!(chunk.code.iter().any(|op| matches!(op, Op::Jump(_))));
    }

    #[test]
    fn emits_function_closure_and_callvalue() {
        let src = "fn add(a: i32, b: i32) -> i32 { return a + b }; f: fn(i32, i32) -> i32 = add; print(f(1, 2))";
        let lex_output = lex(FileId::from_u32(34), src);
        let (ast, parser_diagnostics) = parse(FileId::from_u32(34), src.len() as u32, lex_output.tokens);
        assert!(!parser_diagnostics.has_errors());
        let (hir, mut symbols, lowering_diagnostics) = lower(&ast);
        assert!(!lowering_diagnostics.has_errors());
        let (model, analysis_diagnostics) = analyze(&hir, &mut symbols);
        assert!(!analysis_diagnostics.has_errors());
        let (chunk, compile_diagnostics) = compile_program(&hir, &model);
        assert!(!compile_diagnostics.has_errors());
        assert!(chunk.code.iter().any(|op| matches!(op, Op::MakeClosure(_))));
        assert!(chunk.code.iter().any(|op| matches!(op, Op::CallValue(_))));
    }

    #[test]
    fn emits_tuple_ops() {
        let src = r#"t: (i32, str) = (1, "a"); x := t.0"#;
        let lex_output = lex(FileId::from_u32(35), src);
        let (ast, parser_diagnostics) = parse(FileId::from_u32(35), src.len() as u32, lex_output.tokens);
        assert!(!parser_diagnostics.has_errors());
        let (hir, mut symbols, lowering_diagnostics) = lower(&ast);
        assert!(!lowering_diagnostics.has_errors());
        let (model, analysis_diagnostics) = analyze(&hir, &mut symbols);
        assert!(!analysis_diagnostics.has_errors());
        let (chunk, compile_diagnostics) = compile_program(&hir, &model);
        assert!(!compile_diagnostics.has_errors());
        assert!(chunk.code.iter().any(|op| matches!(op, Op::MakeTuple(_))));
        assert!(chunk.code.iter().any(|op| matches!(op, Op::TupleGet(_))));
    }

    #[test]
    fn emits_make_tuple_for_multi_return_function() {
        let src = "fn pair(a: i32, b: i32) -> (i32, i32) { return a, b }; print(pair(1, 2).0)";
        let lex_output = lex(FileId::from_u32(36), src);
        let (ast, parser_diagnostics) = parse(FileId::from_u32(36), src.len() as u32, lex_output.tokens);
        assert!(!parser_diagnostics.has_errors());
        let (hir, mut symbols, lowering_diagnostics) = lower(&ast);
        assert!(!lowering_diagnostics.has_errors());
        let (model, analysis_diagnostics) = analyze(&hir, &mut symbols);
        assert!(!analysis_diagnostics.has_errors());
        let (chunk, compile_diagnostics) = compile_program(&hir, &model);
        assert!(!compile_diagnostics.has_errors());
        assert!(chunk
            .functions
            .values()
            .any(|fn_chunk| fn_chunk.chunk.code.iter().any(|op| matches!(op, Op::MakeTuple(2)))));
    }
}
