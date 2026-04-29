use std::collections::HashMap;

use crate::{
    analyzer::Type as AnalyzerType,
    hir::{BinOp, Hir, HirExpr, HirId, HirStmt, ScopeId, SymbolId, SymbolTable, UnaryOp},
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
        loop_depth: 0,
        fn_return_stack: Vec::new(),
        self_type_stack: Vec::new(),
        struct_fields: HashMap::new(),
        trait_methods: HashMap::new(),
        impl_methods: HashMap::new(),
    };
    checker.check_program();
    (checker.model, checker.diagnostics)
}

struct Checker<'a> {
    hir: &'a Hir,
    symbols: &'a mut SymbolTable,
    diagnostics: Diagnostics,
    model: SemanticModel,
    loop_depth: usize,
    fn_return_stack: Vec<AnalyzerType>,
    self_type_stack: Vec<AnalyzerType>,
    struct_fields: HashMap<SymbolId, Vec<(String, AnalyzerType)>>,
    trait_methods: HashMap<SymbolId, Vec<(String, Vec<AnalyzerType>, AnalyzerType, bool)>>,
    impl_methods: HashMap<(SymbolId, Option<SymbolId>, String, bool), (Vec<AnalyzerType>, AnalyzerType)>,
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
            HirStmt::FnDecl {
                params,
                return_ty,
                body,
                span,
                ..
            } => {
                self.fn_return_stack.push(return_ty.clone());
                for stmt in body {
                    let _ = self.check_stmt(stmt);
                }
                let _ = self.fn_return_stack.pop();
                if *return_ty != AnalyzerType::Unit && !all_paths_return(body) {
                    self.push_error("function with non-unit return must return on all paths", *span);
                }
                AnalyzerType::Function {
                    params: params
                        .iter()
                        .map(|id| {
                            self.symbols
                                .symbol(*id)
                                .map(|s| s.ty.clone())
                                .unwrap_or(AnalyzerType::Unknown)
                        })
                        .collect(),
                    ret: Box::new(return_ty.clone()),
                }
            }
            HirStmt::StructDecl { symbol, fields, .. } => {
                self.struct_fields.insert(*symbol, fields.clone());
                AnalyzerType::Struct(*symbol)
            }
            HirStmt::TraitDecl { symbol, methods, .. } => {
                self.trait_methods.insert(*symbol, methods.clone());
                AnalyzerType::Trait(*symbol)
            }
            HirStmt::ImplBlock {
                target,
                trait_target,
                methods,
                span,
            } => {
                let resolved_target = resolve_self_type(target.clone(), target.clone());
                self.self_type_stack.push(resolved_target.clone());
                for method_stmt in methods {
                    if let HirStmt::FnDecl { params, .. } = method_stmt {
                        for param in params {
                            if let Some(sym) = self.symbols.symbol_mut(*param) {
                                if matches!(sym.ty, AnalyzerType::SelfType) {
                                    sym.ty = resolved_target.clone();
                                }
                            }
                        }
                    }
                    let _ = self.check_stmt(method_stmt);
                    if let HirStmt::FnDecl {
                        symbol,
                        params,
                        return_ty,
                        ..
                    } = method_stmt
                    {
                        let (name, param_tys) = {
                            let sym = self.symbols.symbol(*symbol);
                            let name = sym.map(|s| s.name.clone()).unwrap_or_else(|| "<invalid>".to_string());
                            let ptys = params
                                .iter()
                                .map(|id| {
                                    self.symbols
                                        .symbol(*id)
                                        .map(|s| resolve_self_type(s.ty.clone(), resolved_target.clone()))
                                        .unwrap_or(AnalyzerType::Unknown)
                                })
                                .collect::<Vec<_>>();
                            (name, ptys)
                        };
                        let is_instance = matches!(param_tys.first(), Some(AnalyzerType::SelfType))
                            || matches!(param_tys.first(), Some(t) if *t == resolved_target);
                        let target_sym = match resolved_target {
                            AnalyzerType::Struct(id) => Some(id),
                            _ => None,
                        };
                        let trait_sym = match trait_target {
                            Some(AnalyzerType::Trait(id)) => Some(*id),
                            _ => None,
                        };
                        if let Some(target_sym) = target_sym {
                            self.impl_methods.insert(
                                (target_sym, trait_sym, name, is_instance),
                                (
                                    param_tys
                                        .into_iter()
                                        .map(|t| resolve_self_type(t, resolved_target.clone()))
                                        .collect(),
                                    resolve_self_type(return_ty.clone(), resolved_target.clone()),
                                ),
                            );
                        }
                    }
                }
                let _ = self.self_type_stack.pop();

                if let (AnalyzerType::Struct(sid), Some(AnalyzerType::Trait(tid))) =
                    (resolved_target.clone(), trait_target.clone())
                {
                    if let Some(required_methods) = self.trait_methods.get(&tid).cloned() {
                        for (name, params, ret, is_instance) in required_methods {
                            let Some((impl_params, impl_ret)) =
                                self.impl_methods.get(&(sid, Some(tid), name.clone(), is_instance))
                            else {
                                self.push_error(
                                    format!("trait impl missing required method '{name}'"),
                                    *span,
                                );
                                continue;
                            };
                            let expected_params = params
                                .into_iter()
                                .map(|t| resolve_self_type(t, AnalyzerType::Struct(sid)))
                                .collect::<Vec<_>>();
                            let expected_ret = resolve_self_type(ret, AnalyzerType::Struct(sid));
                            if *impl_params != expected_params || *impl_ret != expected_ret {
                                self.push_error(
                                    format!("trait method '{name}' signature mismatch"),
                                    *span,
                                );
                            }
                        }
                    }
                }
                AnalyzerType::Unknown
            }
            HirStmt::TupleDestructure { names, ty, value, span } => {
                let value_ty = self.check_expr(*value, *span);
                let tuple_items = match value_ty {
                    AnalyzerType::Tuple(items) => items,
                    AnalyzerType::Unknown => Vec::new(),
                    other => {
                        self.push_error(
                            format!("tuple destructuring requires tuple value, got {other:?}"),
                            *span,
                        );
                        Vec::new()
                    }
                };
                if !tuple_items.is_empty() && tuple_items.len() != names.len() {
                    self.push_error(
                        format!(
                            "tuple destructuring arity mismatch: pattern has {}, value has {}",
                            names.len(),
                            tuple_items.len()
                        ),
                        *span,
                    );
                }
                if let Some(annotated) = ty {
                    if !is_assignable(annotated, &AnalyzerType::Tuple(tuple_items.clone())) {
                        self.push_error("tuple destructuring type annotation mismatch", *span);
                    }
                }
                for (idx, symbol_id) in names.iter().enumerate() {
                    let item_ty = tuple_items.get(idx).cloned().unwrap_or(AnalyzerType::Unknown);
                    if let Some(sym) = self.symbols.symbol_mut(*symbol_id) {
                        sym.ty = item_ty;
                    }
                }
                AnalyzerType::Unknown
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
            HirStmt::ArrayAssign {
                symbol,
                index,
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
                let elem_ty = match expected.0.clone() {
                    AnalyzerType::Array(item) => *item,
                    AnalyzerType::Unknown => AnalyzerType::Unknown,
                    other => {
                        self.push_error(
                            format!("index assignment requires array target, got {other:?}"),
                            *span,
                        );
                        AnalyzerType::Unknown
                    }
                };
                let index_ty = self.check_expr(*index, *span);
                if !matches!(index_ty, AnalyzerType::Int { .. } | AnalyzerType::Unknown) {
                    self.push_error(
                        format!("array index must be integer, got {index_ty:?}"),
                        *span,
                    );
                }
                let actual = self.check_expr_expected(*value, *span, Some(&elem_ty));
                if !is_assignable(&elem_ty, &actual) {
                    self.push_error(
                        format!("cannot assign value of type {actual:?} to array element {elem_ty:?}"),
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
            HirStmt::While {
                condition,
                body,
                span,
            } => {
                let cond_ty = self.check_expr(*condition, *span);
                if !is_truthy_falsy_compatible(&cond_ty) && cond_ty != AnalyzerType::Unknown {
                    self.push_error(
                        format!("while condition is not truthy/falsy-compatible: {cond_ty:?}"),
                        *span,
                    );
                }
                self.loop_depth += 1;
                for stmt in body {
                    let _ = self.check_stmt(stmt);
                }
                self.loop_depth = self.loop_depth.saturating_sub(1);
                AnalyzerType::Unknown
            }
            HirStmt::For {
                init,
                condition,
                step,
                body,
                span,
            } => {
                if let Some(init_stmt) = init {
                    let _ = self.check_stmt(init_stmt);
                }
                if let Some(condition_expr) = condition {
                    let cond_ty = self.check_expr(*condition_expr, *span);
                    if !is_truthy_falsy_compatible(&cond_ty) && cond_ty != AnalyzerType::Unknown {
                        self.push_error(
                            format!("for condition is not truthy/falsy-compatible: {cond_ty:?}"),
                            *span,
                        );
                    }
                }
                self.loop_depth += 1;
                for stmt in body {
                    let _ = self.check_stmt(stmt);
                }
                if let Some(step_expr) = step {
                    let _ = self.check_expr(*step_expr, *span);
                }
                self.loop_depth = self.loop_depth.saturating_sub(1);
                AnalyzerType::Unknown
            }
            HirStmt::Break { span } => {
                if self.loop_depth == 0 {
                    self.push_error("break used outside of loop", *span);
                }
                AnalyzerType::Unknown
            }
            HirStmt::Continue { span } => {
                if self.loop_depth == 0 {
                    self.push_error("continue used outside of loop", *span);
                }
                AnalyzerType::Unknown
            }
            HirStmt::Return { values, span } => {
                let expected_return = self.fn_return_stack.last().cloned();
                let Some(expected_return) = expected_return else {
                    self.push_error("return used outside of function", *span);
                    return AnalyzerType::Unknown;
                };
                if values.is_empty() {
                    if expected_return != AnalyzerType::Unit {
                        self.push_error("return without value requires unit return type", *span);
                    }
                    return expected_return;
                }

                if values.len() > 1 {
                    let AnalyzerType::Tuple(items) = expected_return.clone() else {
                        self.push_error(
                            "multiple return values are allowed only for functions returning tuple",
                            *span,
                        );
                        for expr in values {
                            let _ = self.check_expr(*expr, *span);
                        }
                        return expected_return;
                    };
                    if items.len() != values.len() {
                        self.push_error(
                            format!(
                                "tuple return arity mismatch: expected {}, got {}",
                                items.len(),
                                values.len()
                            ),
                            *span,
                        );
                    }
                    for (idx, expr) in values.iter().enumerate() {
                        let expected_item = items.get(idx).cloned().unwrap_or(AnalyzerType::Unknown);
                        let actual_item = self.check_expr_expected(*expr, *span, Some(&expected_item));
                        if !is_assignable(&expected_item, &actual_item) {
                            self.push_error(
                                format!(
                                    "tuple return position {} type mismatch: expected {expected_item:?}, got {actual_item:?}",
                                    idx
                                ),
                                *span,
                            );
                        }
                    }
                    return expected_return;
                }

                let expr = values[0];
                if matches!(expected_return, AnalyzerType::Tuple(_))
                    && !matches!(self.hir.exprs.get(expr), Some(HirExpr::Tuple(_)))
                {
                    self.push_error(
                        "tuple return must use explicit positional values (e.g. return a, b)",
                        *span,
                    );
                    let _ = self.check_expr(expr, *span);
                    return expected_return;
                }

                let actual = self.check_expr(expr, *span);
                if !is_assignable(&expected_return, &actual) {
                    self.push_error(
                        format!("return type mismatch: expected {expected_return:?}, got {actual:?}"),
                        *span,
                    );
                }
                expected_return
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
            Some(HirExpr::Null) => AnalyzerType::Null,
            Some(HirExpr::Str(_)) => AnalyzerType::Str,
            Some(HirExpr::Char(_)) => AnalyzerType::Char,
            Some(HirExpr::Var(symbol_id)) => self
                .symbols
                .symbol(*symbol_id)
                .map(|s| s.ty.clone())
                .unwrap_or(AnalyzerType::Unknown),
            Some(HirExpr::StructLiteral { type_name, fields }) => {
                let struct_ty = self.resolve_named_type(type_name);
                let AnalyzerType::Struct(symbol) = struct_ty.clone() else {
                    self.push_error(format!("unknown struct type '{type_name}'"), span);
                    return AnalyzerType::Unknown;
                };
                let Some(declared_fields) = self.struct_fields.get(&symbol).cloned() else {
                    self.push_error(format!("unknown struct '{}'", type_name), span);
                    return AnalyzerType::Unknown;
                };
                for (name, expr) in fields {
                    let expected = declared_fields
                        .iter()
                        .find(|(fname, _)| fname == name)
                        .map(|(_, t)| t.clone());
                    let Some(expected_ty) = expected else {
                        self.push_error(format!("unknown field '{name}' for struct '{type_name}'"), span);
                        continue;
                    };
                    let actual = self.check_expr_expected(*expr, span, Some(&expected_ty));
                    if !is_assignable(&expected_ty, &actual) {
                        self.push_error(
                            format!("field '{name}' expects {expected_ty:?}, got {actual:?}"),
                            span,
                        );
                    }
                }
                for (decl_name, _) in &declared_fields {
                    if !fields.iter().any(|(name, _)| name == decl_name) {
                        self.push_error(
                            format!("missing field '{decl_name}' in struct literal '{type_name}'"),
                            span,
                        );
                    }
                }
                AnalyzerType::Struct(symbol)
            }
            Some(HirExpr::FieldAccess { base, field }) => {
                let base_ty = self.check_expr(*base, span);
                match base_ty {
                    AnalyzerType::Err => match field.as_str() {
                        "message" => AnalyzerType::Str,
                        "code" => AnalyzerType::Int {
                            signed: true,
                            bits: 32,
                        },
                        "origin" => AnalyzerType::Str,
                        "cause" => AnalyzerType::Err,
                        _ => {
                            self.push_error(format!("unknown err field '{field}'"), span);
                            AnalyzerType::Unknown
                        }
                    },
                    AnalyzerType::Struct(symbol) => {
                        let Some(fields) = self.struct_fields.get(&symbol) else {
                            self.push_error("unknown struct for field access", span);
                            return AnalyzerType::Unknown;
                        };
                        fields
                            .iter()
                            .find(|(name, _)| name == field)
                            .map(|(_, ty)| ty.clone())
                            .unwrap_or_else(|| {
                                self.push_error(format!("unknown field '{field}'"), span);
                                AnalyzerType::Unknown
                            })
                    }
                    _ => {
                        self.push_error("field access requires struct value", span);
                        AnalyzerType::Unknown
                    }
                }
            }
            Some(HirExpr::MethodCall {
                receiver,
                method,
                args,
            }) => {
                let recv_ty = self.check_expr(*receiver, span);
                let AnalyzerType::Struct(struct_id) = recv_ty.clone() else {
                    self.push_error("instance method call requires struct receiver", span);
                    return AnalyzerType::Unknown;
                };
                let candidates = self
                    .impl_methods
                    .iter()
                    .filter(|((sid, _, name, is_instance), _)| {
                        *sid == struct_id && *name == *method && *is_instance
                    })
                    .map(|(_, sig)| sig.clone())
                    .collect::<Vec<_>>();
                let Some((param_tys, ret_ty)) = candidates.first().cloned() else {
                    self.push_error(format!("unknown method '{}'", method), span);
                    return AnalyzerType::Unknown;
                };
                let expected_args = &param_tys[1..];
                if expected_args.len() != args.len() {
                    self.push_error(
                        format!(
                            "invalid argument count for method '{}': expected {}, got {}",
                            method,
                            expected_args.len(),
                            args.len()
                        ),
                        span,
                    );
                }
                for (idx, arg) in args.iter().enumerate() {
                    let expected = expected_args.get(idx).cloned().unwrap_or(AnalyzerType::Unknown);
                    let actual = self.check_expr_expected(*arg, span, Some(&expected));
                    if !is_assignable(&expected, &actual) {
                        self.push_error(
                            format!("invalid argument type at position {idx}: expected {expected:?}, got {actual:?}"),
                            span,
                        );
                    }
                }
                ret_ty
            }
            Some(HirExpr::StaticMethodCall {
                type_name,
                method,
                args,
            }) => {
                let ty = self.resolve_named_type(type_name);
                let AnalyzerType::Struct(struct_id) = ty else {
                    self.push_error(format!("unknown struct type '{type_name}'"), span);
                    return AnalyzerType::Unknown;
                };
                let candidates = self
                    .impl_methods
                    .iter()
                    .filter(|((sid, _, name, is_instance), _)| {
                        *sid == struct_id && *name == *method && !*is_instance
                    })
                    .map(|(_, sig)| sig.clone())
                    .collect::<Vec<_>>();
                let Some((param_tys, ret_ty)) = candidates.first().cloned() else {
                    self.push_error(format!("unknown static method '{}::{}'", type_name, method), span);
                    return AnalyzerType::Unknown;
                };
                if param_tys.len() != args.len() {
                    self.push_error(
                        format!(
                            "invalid argument count for static method '{}::{}': expected {}, got {}",
                            type_name,
                            method,
                            param_tys.len(),
                            args.len()
                        ),
                        span,
                    );
                }
                for (idx, arg) in args.iter().enumerate() {
                    let expected = param_tys.get(idx).cloned().unwrap_or(AnalyzerType::Unknown);
                    let actual = self.check_expr_expected(*arg, span, Some(&expected));
                    if !is_assignable(&expected, &actual) {
                        self.push_error(
                            format!("invalid argument type at position {idx}: expected {expected:?}, got {actual:?}"),
                            span,
                        );
                    }
                }
                ret_ty
            }
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
                if let Some(name) = self.builtin_name_for_callee(*callee) {
                    return self.check_special_builtin_contract(&name, args, span);
                }
                let callee_ty = self.check_expr(*callee, span);
                self.check_call(callee_ty, args, span)
            }
            Some(HirExpr::Tuple(items)) => {
                let item_tys = items
                    .iter()
                    .map(|item| self.check_expr(*item, span))
                    .collect::<Vec<_>>();
                AnalyzerType::Tuple(item_tys)
            }
            Some(HirExpr::TupleAccess { tuple, index }) => {
                let tuple_ty = self.check_expr(*tuple, span);
                match tuple_ty {
                    AnalyzerType::Tuple(items) => match items.get(*index) {
                        Some(item_ty) => item_ty.clone(),
                        None => {
                            self.push_error(
                                format!("tuple index {} out of range (len={})", index, items.len()),
                                span,
                            );
                            AnalyzerType::Unknown
                        }
                    },
                    AnalyzerType::Unknown => AnalyzerType::Unknown,
                    other => {
                        self.push_error(
                            format!("tuple access requires tuple value, got {other:?}"),
                            span,
                        );
                        AnalyzerType::Unknown
                    }
                }
            }
            Some(HirExpr::Array(items)) => {
                let expected_item = match expected {
                    Some(AnalyzerType::Array(item)) => Some((**item).clone()),
                    _ => None,
                };
                if items.is_empty() {
                    if let Some(item) = expected_item {
                        AnalyzerType::Array(Box::new(item))
                    } else {
                        self.push_error("empty array literal requires explicit array type context", span);
                        AnalyzerType::Unknown
                    }
                } else {
                    let mut inferred = expected_item.unwrap_or_else(|| self.check_expr(items[0], span));
                    for item in items {
                        let actual = self.check_expr_expected(*item, span, Some(&inferred));
                        if !is_assignable(&inferred, &actual) {
                            if matches!(inferred, AnalyzerType::Unknown) {
                                inferred = actual;
                            } else {
                                self.push_error(
                                    format!(
                                        "array literal item type mismatch: expected {inferred:?}, got {actual:?}"
                                    ),
                                    span,
                                );
                            }
                        }
                    }
                    AnalyzerType::Array(Box::new(inferred))
                }
            }
            Some(HirExpr::ArrayAccess { array, index }) => {
                let array_ty = self.check_expr(*array, span);
                let index_ty = self.check_expr(*index, span);
                if !matches!(index_ty, AnalyzerType::Int { .. } | AnalyzerType::Unknown) {
                    self.push_error(
                        format!("array index must be integer, got {index_ty:?}"),
                        span,
                    );
                }
                match array_ty {
                    AnalyzerType::Array(item) => *item,
                    AnalyzerType::Tuple(items) => {
                        if let Some(HirExpr::Int(raw)) = self.hir.exprs.get(*index) {
                            let Ok(idx) = raw.parse::<usize>() else {
                                self.push_error("tuple index must be integer literal", span);
                                return AnalyzerType::Unknown;
                            };
                            items.get(idx).cloned().unwrap_or_else(|| {
                                self.push_error(
                                    format!("tuple index {} out of range (len={})", idx, items.len()),
                                    span,
                                );
                                AnalyzerType::Unknown
                            })
                        } else {
                            self.push_error("tuple index must be integer literal", span);
                            AnalyzerType::Unknown
                        }
                    }
                    AnalyzerType::Unknown => AnalyzerType::Unknown,
                    other => {
                        self.push_error(
                            format!("index access requires array/tuple value, got {other:?}"),
                            span,
                        );
                        AnalyzerType::Unknown
                    }
                }
            }
            Some(HirExpr::Propagate { expr }) => {
                let inner_ty = self.check_expr(*expr, span);
                let Some((ok_ty, err_ty)) = extract_fallible_tuple(&inner_ty) else {
                    self.push_error(
                        format!("operator '?' expects expression of type (T, err), got {inner_ty:?}"),
                        span,
                    );
                    return AnalyzerType::Unknown;
                };
                if !is_error_like(&err_ty, &self.struct_fields) {
                    self.push_error(
                        format!("operator '?' expects tuple error position to be err-like, got {err_ty:?}"),
                        span,
                    );
                    return AnalyzerType::Unknown;
                }
                let Some(current_ret) = self.fn_return_stack.last() else {
                    self.push_error("operator '?' can only be used inside function body", span);
                    return AnalyzerType::Unknown;
                };
                let Some((_, current_err_ty)) = extract_fallible_tuple(current_ret) else {
                    self.push_error(
                        format!(
                            "operator '?' requires current function return type to be (T, err), got {current_ret:?}"
                        ),
                        span,
                    );
                    return AnalyzerType::Unknown;
                };
                if !is_error_like(&current_err_ty, &self.struct_fields) {
                    self.push_error(
                        format!(
                            "operator '?' requires current function error type to be err-like, got {current_err_ty:?}"
                        ),
                        span,
                    );
                    return AnalyzerType::Unknown;
                }
                if !is_assignable(&current_err_ty, &err_ty) {
                    self.push_error(
                        format!(
                            "cannot propagate error type {err_ty:?} from '?' into function error type {current_err_ty:?}"
                        ),
                        span,
                    );
                    return AnalyzerType::Unknown;
                }
                ok_ty
            }
            Some(HirExpr::TryCatch {
                try_expr,
                err_symbol,
                catch_stmts,
                catch_value,
            }) => {
                let try_ty = self.check_expr(*try_expr, span);
                let Some((ok_ty, err_ty)) = extract_fallible_tuple(&try_ty) else {
                    self.push_error(
                        format!("try expression expects value of type (T, err), got {try_ty:?}"),
                        span,
                    );
                    return AnalyzerType::Unknown;
                };
                if !is_error_like(&err_ty, &self.struct_fields) {
                    self.push_error(
                        format!("try expression expects tuple error position to be err-like, got {err_ty:?}"),
                        span,
                    );
                    return AnalyzerType::Unknown;
                }
                let binding_ty = self
                    .symbols
                    .symbol(*err_symbol)
                    .map(|s| s.ty.clone())
                    .unwrap_or(AnalyzerType::Unknown);
                if !is_error_like(&binding_ty, &self.struct_fields) && binding_ty != AnalyzerType::Unknown {
                    self.push_error("catch binding type must be err-like", span);
                }
                if !is_assignable(&binding_ty, &err_ty) && binding_ty != AnalyzerType::Unknown {
                    self.push_error(
                        format!("catch binding type {binding_ty:?} does not match {err_ty:?}"),
                        span,
                    );
                }
                for stmt in catch_stmts {
                    let _ = self.check_stmt(stmt);
                }
                let catch_ty = self.check_expr_expected(*catch_value, span, Some(&ok_ty));
                if !is_assignable(&ok_ty, &catch_ty) {
                    self.push_error(
                        format!(
                            "catch expression type mismatch: expected {ok_ty:?}, got {catch_ty:?}"
                        ),
                        span,
                    );
                    AnalyzerType::Unknown
                } else {
                    ok_ty
                }
            }
            Some(HirExpr::IncDec {
                symbol,
                op: _,
                position: _,
            }) => {
                let Some(sym) = self.symbols.symbol(*symbol) else {
                    self.push_error("invalid increment/decrement target", span);
                    return AnalyzerType::Unknown;
                };
                if sym.is_const {
                    self.push_error("cannot assign to constant", span);
                    return AnalyzerType::Unknown;
                }
                if is_numeric_type(&sym.ty) {
                    sym.ty.clone()
                } else {
                    self.push_error(
                        format!("increment/decrement requires numeric variable, got {:?}", sym.ty),
                        span,
                    );
                    AnalyzerType::Unknown
                }
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
            BinOp::Add => {
                if matches!(left_ty, AnalyzerType::Str) || matches!(right_ty, AnalyzerType::Str) {
                    AnalyzerType::Str
                } else {
                    self.check_numeric_pair(op, left_ty, right_ty, span)
                }
            }
            BinOp::Subtract | BinOp::Multiply | BinOp::Divide | BinOp::Modulo | BinOp::Power => {
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
        if left_ty == right_ty || is_assignable(&left_ty, &right_ty) || is_assignable(&right_ty, &left_ty) {
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

    fn builtin_name_for_callee(&self, callee: HirId) -> Option<String> {
        let HirExpr::Var(symbol_id) = self.hir.exprs.get(callee)? else {
            return None;
        };
        let sym = self.symbols.symbol(*symbol_id)?;
        (sym.origin == crate::hir::SymbolOrigin::Builtin).then(|| sym.name.clone())
    }

    fn check_special_builtin_contract(
        &mut self,
        name: &str,
        args: &[HirId],
        span: Span,
    ) -> AnalyzerType {
        match name {
            "error" => {
                if args.len() != 1 && args.len() != 2 {
                    self.push_error(
                        format!("error expects 1 or 2 arguments, got {}", args.len()),
                        span,
                    );
                }
                if let Some(first) = args.first() {
                    let msg_ty = self.check_expr_expected(*first, span, Some(&AnalyzerType::Str));
                    if !is_assignable(&AnalyzerType::Str, &msg_ty) {
                        self.push_error(
                            format!("error message must be str, got {msg_ty:?}"),
                            span,
                        );
                    }
                }
                if let Some(second) = args.get(1) {
                    let expected_code = AnalyzerType::Int {
                        signed: true,
                        bits: 32,
                    };
                    let code_ty = self.check_expr_expected(*second, span, Some(&expected_code));
                    if !is_assignable(&expected_code, &code_ty) {
                        self.push_error(
                            format!("error code must be i32, got {code_ty:?}"),
                            span,
                        );
                    }
                }
                AnalyzerType::Err
            }
            "panic" => {
                if args.len() != 1 && args.len() != 2 {
                    self.push_error(
                        format!("panic expects 1 or 2 arguments, got {}", args.len()),
                        span,
                    );
                }
                if let Some(first) = args.first() {
                    let msg_ty = self.check_expr_expected(*first, span, Some(&AnalyzerType::Str));
                    if !is_assignable(&AnalyzerType::Str, &msg_ty) {
                        self.push_error(
                            format!("panic message must be str, got {msg_ty:?}"),
                            span,
                        );
                    }
                }
                if let Some(second) = args.get(1) {
                    let expected_code = AnalyzerType::Int {
                        signed: true,
                        bits: 32,
                    };
                    let code_ty = self.check_expr_expected(*second, span, Some(&expected_code));
                    if !is_assignable(&expected_code, &code_ty) {
                        self.push_error(
                            format!("panic code must be i32, got {code_ty:?}"),
                            span,
                        );
                    }
                }
                AnalyzerType::Unit
            }
            "wrap" => {
                if args.len() != 2 && args.len() != 3 {
                    self.push_error(
                        format!("wrap expects 2 or 3 arguments, got {}", args.len()),
                        span,
                    );
                }
                if let Some(first) = args.first() {
                    let err_ty = self.check_expr(*first, span);
                    if !is_error_like(&err_ty, &self.struct_fields) && err_ty != AnalyzerType::Unknown {
                        self.push_error(
                            format!("wrap first argument must be err-like, got {err_ty:?}"),
                            span,
                        );
                    }
                }
                if let Some(second) = args.get(1) {
                    let msg_ty = self.check_expr_expected(*second, span, Some(&AnalyzerType::Str));
                    if !is_assignable(&AnalyzerType::Str, &msg_ty) {
                        self.push_error(
                            format!("wrap message must be str, got {msg_ty:?}"),
                            span,
                        );
                    }
                }
                if let Some(third) = args.get(2) {
                    let expected_code = AnalyzerType::Int {
                        signed: true,
                        bits: 32,
                    };
                    let code_ty = self.check_expr_expected(*third, span, Some(&expected_code));
                    if !is_assignable(&expected_code, &code_ty) {
                        self.push_error(
                            format!("wrap code must be i32, got {code_ty:?}"),
                            span,
                        );
                    }
                }
                AnalyzerType::Err
            }
            "len" => {
                if args.len() != 1 {
                    self.push_error(
                        format!("len expects exactly 1 argument, got {}", args.len()),
                        span,
                    );
                    return AnalyzerType::Unknown;
                }
                let arg_ty = self.check_expr(args[0], span);
                match arg_ty {
                    AnalyzerType::Str | AnalyzerType::Array(_) | AnalyzerType::Unknown => AnalyzerType::Int {
                        signed: false,
                        bits: 64,
                    },
                    other => {
                        self.push_error(format!("len expects str or array, got {other:?}"), span);
                        AnalyzerType::Unknown
                    }
                }
            }
            _ => {
                // fall through to generic behavior for other builtins
                let callee_ty = self.resolve_named_type(name);
                self.check_call(callee_ty, args, span)
            }
        }
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

    fn resolve_named_type(&self, name: &str) -> AnalyzerType {
        self.symbols
            .resolve(ScopeId(0), name)
            .and_then(|id| self.symbols.symbol(id))
            .map(|s| s.ty.clone())
            .unwrap_or(AnalyzerType::Unknown)
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
    if expected == actual
        || matches!(expected, AnalyzerType::Unknown | AnalyzerType::Any)
        || matches!(actual, AnalyzerType::Unknown | AnalyzerType::Null)
    {
        return true;
    }
    match (expected, actual) {
        (AnalyzerType::Tuple(expected_items), AnalyzerType::Tuple(actual_items)) => {
            expected_items.len() == actual_items.len()
                && expected_items
                    .iter()
                    .zip(actual_items.iter())
                    .all(|(e, a)| is_assignable(e, a))
        }
        (AnalyzerType::Array(expected_item), AnalyzerType::Array(actual_item)) => {
            is_assignable(expected_item, actual_item)
        }
        _ => false,
    }
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

fn is_numeric_type(ty: &AnalyzerType) -> bool {
    matches!(ty, AnalyzerType::Int { .. } | AnalyzerType::Float { .. })
}

fn is_error_like(
    ty: &AnalyzerType,
    struct_fields: &HashMap<SymbolId, Vec<(String, AnalyzerType)>>,
) -> bool {
    match ty {
        AnalyzerType::Err => true,
        AnalyzerType::Struct(symbol_id) => {
            let Some(fields) = struct_fields.get(symbol_id) else {
                return false;
            };
            let has_message = fields
                .iter()
                .any(|(name, ty)| name == "message" && *ty == AnalyzerType::Str);
            let has_code = fields.iter().any(|(name, ty)| {
                name == "code"
                    && *ty
                        == AnalyzerType::Int {
                            signed: true,
                            bits: 32,
                        }
            });
            has_message && has_code
        }
        _ => false,
    }
}

fn extract_fallible_tuple(ty: &AnalyzerType) -> Option<(AnalyzerType, AnalyzerType)> {
    match ty {
        AnalyzerType::Tuple(items) if items.len() == 2 => Some((items[0].clone(), items[1].clone())),
        _ => None,
    }
}

fn all_paths_return(stmts: &[HirStmt]) -> bool {
    let Some(last) = stmts.last() else {
        return false;
    };
    match last {
        HirStmt::Return { .. } => true,
        HirStmt::Block { stmts, .. } => all_paths_return(stmts),
        HirStmt::If {
            then_branch,
            else_branch: Some(else_branch),
            ..
        } => all_paths_return(then_branch) && all_paths_return(else_branch),
        _ => false,
    }
}

fn resolve_self_type(ty: AnalyzerType, concrete: AnalyzerType) -> AnalyzerType {
    match ty {
        AnalyzerType::SelfType => concrete,
        AnalyzerType::Function { params, ret } => AnalyzerType::Function {
            params: params
                .into_iter()
                .map(|p| resolve_self_type(p, concrete.clone()))
                .collect(),
            ret: Box::new(resolve_self_type(*ret, concrete)),
        },
        AnalyzerType::Tuple(items) => AnalyzerType::Tuple(
            items
                .into_iter()
                .map(|i| resolve_self_type(i, concrete.clone()))
                .collect(),
        ),
        AnalyzerType::Array(item) => AnalyzerType::Array(Box::new(resolve_self_type(*item, concrete))),
        other => other,
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
    fn question_requires_fallible_function_return() {
        let src = "fn div(a: i32, b: i32) -> (i32, err) { return a / b, null }\nfn bad() -> i32 { return div(1, 1)? }";
        let lex_output = lex(FileId::from_u32(61), src);
        let (ast, _) = parse(FileId::from_u32(61), src.len() as u32, lex_output.tokens);
        let (hir, mut symbols, _) = lower(&ast);
        let (_model, diagnostics) = analyze(&hir, &mut symbols);
        assert!(diagnostics.has_errors());
        assert!(diagnostics
            .iter()
            .any(|d| d.message.contains("operator '?' requires current function return type to be (T, err)")));
    }

    #[test]
    fn supports_domain_error_type_in_try_catch() {
        let src = r#"
            struct PaymentError { message: str, code: i32, detail: str }
            op_err: PaymentError = PaymentError { message: "x", code: 1, detail: "d" }
            pair := (0, op_err)
            v := try pair catch(e: PaymentError) { return 0 }
        "#;
        let lex_output = lex(FileId::from_u32(62), src);
        let (ast, _) = parse(FileId::from_u32(62), src.len() as u32, lex_output.tokens);
        let (hir, mut symbols, _) = lower(&ast);
        let (_model, diagnostics) = analyze(&hir, &mut symbols);
        assert!(!diagnostics.has_errors());
    }

    #[test]
    fn supports_array_types_indexing_and_len() {
        let src = "arr: [i32] = [1, 2, 3]; x: i32 = arr[1]; arr[0] = 9; y := len(arr)";
        let lex_output = lex(FileId::from_u32(620), src);
        let (ast, _) = parse(FileId::from_u32(620), src.len() as u32, lex_output.tokens);
        let (hir, mut symbols, _) = lower(&ast);
        let (_model, diagnostics) = analyze(&hir, &mut symbols);
        assert!(!diagnostics.has_errors());
    }

    #[test]
    fn rejects_invalid_array_index_type() {
        let src = "arr: [i32] = [1, 2, 3]; x := arr[true]";
        let lex_output = lex(FileId::from_u32(621), src);
        let (ast, _) = parse(FileId::from_u32(621), src.len() as u32, lex_output.tokens);
        let (hir, mut symbols, _) = lower(&ast);
        let (_model, diagnostics) = analyze(&hir, &mut symbols);
        assert!(diagnostics.has_errors());
        assert!(diagnostics.iter().any(|d| d.message.contains("array index must be integer")));
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

    #[test]
    fn accepts_truthy_while_condition() {
        let src = "while 1 { break }";
        let lex_output = lex(FileId::from_u32(35), src);
        let (ast, _) = parse(FileId::from_u32(35), src.len() as u32, lex_output.tokens);
        let (hir, mut symbols, _) = lower(&ast);
        let (_model, diagnostics) = analyze(&hir, &mut symbols);
        assert!(!diagnostics.has_errors());
    }

    #[test]
    fn rejects_break_outside_loop_semantically() {
        let src = "break";
        let lex_output = lex(FileId::from_u32(36), src);
        let (ast, _) = parse(FileId::from_u32(36), src.len() as u32, lex_output.tokens);
        let (hir, mut symbols, _) = lower(&ast);
        let (_model, diagnostics) = analyze(&hir, &mut symbols);
        assert!(diagnostics.iter().any(|d| d.message.contains("break used outside of loop")));
    }

    #[test]
    fn rejects_continue_outside_loop_semantically() {
        let src = "continue";
        let lex_output = lex(FileId::from_u32(37), src);
        let (ast, _) = parse(FileId::from_u32(37), src.len() as u32, lex_output.tokens);
        let (hir, mut symbols, _) = lower(&ast);
        let (_model, diagnostics) = analyze(&hir, &mut symbols);
        assert!(diagnostics
            .iter()
            .any(|d| d.message.contains("continue used outside of loop")));
    }

    #[test]
    fn accepts_string_plus_int_concatenation() {
        let src = r#"s := "hello"; r := s + 42"#;
        let lex_output = lex(FileId::from_u32(38), src);
        let (ast, _) = parse(FileId::from_u32(38), src.len() as u32, lex_output.tokens);
        let (hir, mut symbols, _) = lower(&ast);
        let (_model, diagnostics) = analyze(&hir, &mut symbols);
        assert!(!diagnostics.has_errors());
    }

    #[test]
    fn accepts_int_plus_string_concatenation() {
        let src = r#"r := 42 + "hello""#;
        let lex_output = lex(FileId::from_u32(39), src);
        let (ast, _) = parse(FileId::from_u32(39), src.len() as u32, lex_output.tokens);
        let (hir, mut symbols, _) = lower(&ast);
        let (_model, diagnostics) = analyze(&hir, &mut symbols);
        assert!(!diagnostics.has_errors());
    }

    #[test]
    fn accepts_string_compound_assign() {
        let src = r#"s := "hello"; s += " world""#;
        let lex_output = lex(FileId::from_u32(40), src);
        let (ast, _) = parse(FileId::from_u32(40), src.len() as u32, lex_output.tokens);
        let (hir, mut symbols, _) = lower(&ast);
        let (_model, diagnostics) = analyze(&hir, &mut symbols);
        assert!(!diagnostics.has_errors());
    }

    #[test]
    fn accepts_string_plus_bool_concatenation() {
        let src = r#"r := "value: " + true"#;
        let lex_output = lex(FileId::from_u32(41), src);
        let (ast, _) = parse(FileId::from_u32(41), src.len() as u32, lex_output.tokens);
        let (hir, mut symbols, _) = lower(&ast);
        let (_model, diagnostics) = analyze(&hir, &mut symbols);
        assert!(!diagnostics.has_errors());
    }

    #[test]
    fn rejects_compound_assign_to_const() {
        let src = "x :: i32 = 1; x += 1";
        let lex_output = lex(FileId::from_u32(42), src);
        let (ast, _) = parse(FileId::from_u32(42), src.len() as u32, lex_output.tokens);
        let (hir, mut symbols, _) = lower(&ast);
        let (_model, diagnostics) = analyze(&hir, &mut symbols);
        assert!(diagnostics.has_errors());
        assert!(diagnostics.iter().any(|d| d.message.contains("cannot assign to constant")));
    }

    #[test]
    fn accepts_for_loop_with_incdec_step() {
        let src = "for i: i32 = 0; i < 3; i++ { print(i) }";
        let lex_output = lex(FileId::from_u32(43), src);
        let (ast, _) = parse(FileId::from_u32(43), src.len() as u32, lex_output.tokens);
        let (hir, mut symbols, _) = lower(&ast);
        let (_model, diagnostics) = analyze(&hir, &mut symbols);
        assert!(!diagnostics.has_errors());
    }

    #[test]
    fn rejects_incdec_on_non_numeric_type() {
        let src = r#"s: str = "a"; s++"#;
        let lex_output = lex(FileId::from_u32(44), src);
        let (ast, _) = parse(FileId::from_u32(44), src.len() as u32, lex_output.tokens);
        let (hir, mut symbols, _) = lower(&ast);
        let (_model, diagnostics) = analyze(&hir, &mut symbols);
        assert!(diagnostics.has_errors());
        assert!(diagnostics
            .iter()
            .any(|d| d.message.contains("increment/decrement requires numeric variable")));
    }

    #[test]
    fn rejects_incdec_on_const() {
        let src = "x:: i32 = 1; ++x";
        let lex_output = lex(FileId::from_u32(45), src);
        let (ast, _) = parse(FileId::from_u32(45), src.len() as u32, lex_output.tokens);
        let (hir, mut symbols, _) = lower(&ast);
        let (_model, diagnostics) = analyze(&hir, &mut symbols);
        assert!(diagnostics.has_errors());
        assert!(diagnostics.iter().any(|d| d.message.contains("cannot assign to constant")));
    }

    #[test]
    fn rejects_return_outside_function() {
        let src = "return 1";
        let lex_output = lex(FileId::from_u32(46), src);
        let (ast, _) = parse(FileId::from_u32(46), src.len() as u32, lex_output.tokens);
        let (hir, mut symbols, _) = lower(&ast);
        let (_model, diagnostics) = analyze(&hir, &mut symbols);
        assert!(diagnostics.has_errors());
        assert!(diagnostics
            .iter()
            .any(|d| d.message.contains("return used outside of function")));
    }

    #[test]
    fn rejects_non_unit_function_without_explicit_return() {
        let src = "fn bad(a: i32) -> i32 { a + 1 }";
        let lex_output = lex(FileId::from_u32(47), src);
        let (ast, _) = parse(FileId::from_u32(47), src.len() as u32, lex_output.tokens);
        let (hir, mut symbols, _) = lower(&ast);
        let (_model, diagnostics) = analyze(&hir, &mut symbols);
        assert!(diagnostics.has_errors());
        assert!(diagnostics
            .iter()
            .any(|d| d.message.contains("must return on all paths")));
    }

    #[test]
    fn accepts_function_type_annotation() {
        let src = "fn add(a: i32, b: i32) -> i32 { return a + b }; f: fn(i32, i32) -> i32 = add";
        let lex_output = lex(FileId::from_u32(48), src);
        let (ast, _) = parse(FileId::from_u32(48), src.len() as u32, lex_output.tokens);
        let (hir, mut symbols, _) = lower(&ast);
        let (_model, diagnostics) = analyze(&hir, &mut symbols);
        assert!(!diagnostics.has_errors());
    }

    #[test]
    fn accepts_function_as_argument_and_return_value() {
        let src = "fn inc(x: i32) -> i32 { return x + 1 }; fn apply(f: fn(i32) -> i32, x: i32) -> i32 { return f(x) }; r: i32 = apply(inc, 1)";
        let lex_output = lex(FileId::from_u32(49), src);
        let (ast, _) = parse(FileId::from_u32(49), src.len() as u32, lex_output.tokens);
        let (hir, mut symbols, _) = lower(&ast);
        let (_model, diagnostics) = analyze(&hir, &mut symbols);
        assert!(!diagnostics.has_errors());
    }

    #[test]
    fn accepts_unit_function_with_bare_return() {
        let src = "fn noop() -> unit { return }; noop()";
        let lex_output = lex(FileId::from_u32(50), src);
        let (ast, _) = parse(FileId::from_u32(50), src.len() as u32, lex_output.tokens);
        let (hir, mut symbols, _) = lower(&ast);
        let (_model, diagnostics) = analyze(&hir, &mut symbols);
        assert!(!diagnostics.has_errors());
    }

    #[test]
    fn validates_tuple_destructure_arity() {
        let src = "t: (i32, i32) = (1, 2); (a, b, c) := t";
        let lex_output = lex(FileId::from_u32(51), src);
        let (ast, _) = parse(FileId::from_u32(51), src.len() as u32, lex_output.tokens);
        let (hir, mut symbols, _) = lower(&ast);
        let (_model, diagnostics) = analyze(&hir, &mut symbols);
        assert!(diagnostics.has_errors());
        assert!(diagnostics
            .iter()
            .any(|d| d.message.contains("tuple destructuring arity mismatch")));
    }

    #[test]
    fn validates_tuple_index_range() {
        let src = "t: (i32, i32) = (1, 2); x := t.2";
        let lex_output = lex(FileId::from_u32(52), src);
        let (ast, _) = parse(FileId::from_u32(52), src.len() as u32, lex_output.tokens);
        let (hir, mut symbols, _) = lower(&ast);
        let (_model, diagnostics) = analyze(&hir, &mut symbols);
        assert!(diagnostics.has_errors());
        assert!(diagnostics
            .iter()
            .any(|d| d.message.contains("tuple index 2 out of range")));
    }

    #[test]
    fn allows_null_assignment_to_typed_binding() {
        let src = "x: i32 = null; y: bool = null; z: (i32, i32) = null";
        let lex_output = lex(FileId::from_u32(53), src);
        let (ast, _) = parse(FileId::from_u32(53), src.len() as u32, lex_output.tokens);
        let (hir, mut symbols, _) = lower(&ast);
        let (_model, diagnostics) = analyze(&hir, &mut symbols);
        assert!(!diagnostics.has_errors());
    }

    #[test]
    fn accepts_multi_value_return_for_tuple_function() {
        let src = "fn pair(a: i32, b: i32) -> (i32, i32) { return a, b }";
        let lex_output = lex(FileId::from_u32(54), src);
        let (ast, _) = parse(FileId::from_u32(54), src.len() as u32, lex_output.tokens);
        let (hir, mut symbols, _) = lower(&ast);
        let (_model, diagnostics) = analyze(&hir, &mut symbols);
        assert!(!diagnostics.has_errors());
    }

    #[test]
    fn rejects_multi_value_return_for_non_tuple_function() {
        let src = "fn bad(a: i32, b: i32) -> i32 { return a, b }";
        let lex_output = lex(FileId::from_u32(55), src);
        let (ast, _) = parse(FileId::from_u32(55), src.len() as u32, lex_output.tokens);
        let (hir, mut symbols, _) = lower(&ast);
        let (_model, diagnostics) = analyze(&hir, &mut symbols);
        assert!(diagnostics.has_errors());
        assert!(diagnostics
            .iter()
            .any(|d| d.message.contains("multiple return values are allowed only for functions returning tuple")));
    }

    #[test]
    fn rejects_tuple_function_returning_single_tuple_variable() {
        let src = "fn bad(p: (i32, i32)) -> (i32, i32) { return p }";
        let lex_output = lex(FileId::from_u32(56), src);
        let (ast, _) = parse(FileId::from_u32(56), src.len() as u32, lex_output.tokens);
        let (hir, mut symbols, _) = lower(&ast);
        let (_model, diagnostics) = analyze(&hir, &mut symbols);
        assert!(diagnostics.has_errors());
        assert!(diagnostics
            .iter()
            .any(|d| d.message.contains("tuple return must use explicit positional values")));
    }

    #[test]
    fn allows_tuple_function_return_with_parenthesized_tuple_literal() {
        let src = "fn pair(a: i32, b: i32) -> (i32, i32) { return (a, b) }";
        let lex_output = lex(FileId::from_u32(57), src);
        let (ast, _) = parse(FileId::from_u32(57), src.len() as u32, lex_output.tokens);
        let (hir, mut symbols, _) = lower(&ast);
        let (_model, diagnostics) = analyze(&hir, &mut symbols);
        assert!(!diagnostics.has_errors());
    }

    #[test]
    fn validates_tuple_return_arity_and_position_types() {
        let src = "fn bad(a: i32) -> (i32, bool) { return a, a }";
        let lex_output = lex(FileId::from_u32(58), src);
        let (ast, _) = parse(FileId::from_u32(58), src.len() as u32, lex_output.tokens);
        let (hir, mut symbols, _) = lower(&ast);
        let (_model, diagnostics) = analyze(&hir, &mut symbols);
        assert!(diagnostics.has_errors());
        assert!(diagnostics
            .iter()
            .any(|d| d.message.contains("tuple return position 1 type mismatch")));
    }

    #[test]
    fn allows_null_in_tuple_return_positions() {
        let src = "fn pair(a: i32) -> (i32, bool) { return a, null }";
        let lex_output = lex(FileId::from_u32(59), src);
        let (ast, _) = parse(FileId::from_u32(59), src.len() as u32, lex_output.tokens);
        let (hir, mut symbols, _) = lower(&ast);
        let (_model, diagnostics) = analyze(&hir, &mut symbols);
        assert!(!diagnostics.has_errors());
    }

    #[test]
    fn accepts_destructure_without_parentheses_and_underscore_discard() {
        let src = "fn pair(a: i32, b: bool) -> (i32, bool) { return a, b }; x, _ := pair(1, true)";
        let lex_output = lex(FileId::from_u32(60), src);
        let (ast, _) = parse(FileId::from_u32(60), src.len() as u32, lex_output.tokens);
        let (hir, mut symbols, _) = lower(&ast);
        let (_model, diagnostics) = analyze(&hir, &mut symbols);
        assert!(!diagnostics.has_errors());
    }

    #[test]
    fn accepts_error_builtin_with_default_and_custom_code() {
        let src = r#"e1: err = error("x"); e2: err = error("x", -1)"#;
        let lex_output = lex(FileId::from_u32(61), src);
        let (ast, _) = parse(FileId::from_u32(61), src.len() as u32, lex_output.tokens);
        let (hir, mut symbols, _) = lower(&ast);
        let (_model, diagnostics) = analyze(&hir, &mut symbols);
        assert!(!diagnostics.has_errors());
    }

    #[test]
    fn rejects_error_builtin_invalid_arguments() {
        let src = r#"a := error(1); b := error("x", true); c := error("x", 1, 2)"#;
        let lex_output = lex(FileId::from_u32(62), src);
        let (ast, _) = parse(FileId::from_u32(62), src.len() as u32, lex_output.tokens);
        let (hir, mut symbols, _) = lower(&ast);
        let (_model, diagnostics) = analyze(&hir, &mut symbols);
        assert!(diagnostics.has_errors());
        assert!(diagnostics.iter().any(|d| d.message.contains("error message must be str")));
        assert!(diagnostics.iter().any(|d| d.message.contains("error code must be i32")));
        assert!(diagnostics.iter().any(|d| d.message.contains("error expects 1 or 2 arguments")));
    }

    #[test]
    fn accepts_err_field_access_types() {
        let src = r#"e: err = error("x", 7); m: str = e.message; c: i32 = e.code"#;
        let lex_output = lex(FileId::from_u32(63), src);
        let (ast, _) = parse(FileId::from_u32(63), src.len() as u32, lex_output.tokens);
        let (hir, mut symbols, _) = lower(&ast);
        let (_model, diagnostics) = analyze(&hir, &mut symbols);
        assert!(!diagnostics.has_errors());
    }
}
