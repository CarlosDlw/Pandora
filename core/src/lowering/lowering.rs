use std::collections::HashMap;

use crate::{
    analyzer::Type,
    ast::{
        ArrayItem as AstArrayItem, Ast, AstNode, BinaryOp, CompoundOp, IncDecOp as AstIncDecOp,
        IncDecPosition as AstIncDecPosition, UnaryOp,
    },
    builtins::default_registry,
    hir::{
        BinOp, Hir, HirArrayItem, HirExpr, HirStmt, IncDecOp as HirIncDecOp,
        IncDecPosition as HirIncDecPosition, ScopeId, SymbolId, SymbolOrigin, SymbolTable,
        UnaryOp as HirUnaryOp,
    },
};
use foundation::{
    arena::Arena,
    diagnostics::{Diagnostic, Diagnostics, Severity},
    ids::ArenaId,
    span::Span,
};

pub fn lower(ast: &Ast) -> (Hir, SymbolTable, Diagnostics) {
    lower_with_registry(ast, &default_registry())
}

pub fn lower_with_registry(
    ast: &Ast,
    registry: &crate::builtins::BuiltinRegistry,
) -> (Hir, SymbolTable, Diagnostics) {
    let mut lowering = Lowering::new(ast);
    lowering.init_builtins(registry);
    lowering.lower_program();
    (lowering.hir, lowering.symbols, lowering.diagnostics)
}

struct Lowering<'a> {
    ast: &'a Ast,
    hir: Hir,
    symbols: SymbolTable,
    diagnostics: Diagnostics,
    current_scope: ScopeId,
    predeclared_fns: HashMap<ArenaId, SymbolId>,
    discard_counter: u32,
}

impl<'a> Lowering<'a> {
    fn new(ast: &'a Ast) -> Self {
        let mut symbols = SymbolTable::new();
        let root_scope = symbols.create_scope(None);
        Self {
            ast,
            hir: Hir {
                file_id: ast.file_id,
                exprs: Arena::new(),
                stmts: Vec::new(),
                expr_spans: HashMap::new(),
            },
            symbols,
            diagnostics: Diagnostics::new(),
            current_scope: root_scope,
            predeclared_fns: HashMap::new(),
            discard_counter: 0,
        }
    }

    fn init_builtins(&mut self, registry: &crate::builtins::BuiltinRegistry) {
        init_global_scope(&mut self.symbols, self.current_scope, registry);
    }

    fn lower_program(&mut self) {
        self.predeclare_top_level_functions();
        for root in &self.ast.roots {
            let stmt = self.lower_stmt(*root);
            self.hir.stmts.push(stmt);
        }
    }

    fn predeclare_top_level_functions(&mut self) {
        for root in &self.ast.roots {
            let Some(AstNode::FnDecl {
                name,
                params,
                return_ty,
                ..
            }) = self.ast.get(*root)
            else {
                continue;
            };
            let ret_ty = self.resolve_decl_type(Some(*return_ty));
            let fn_ty = Type::Function {
                params: params
                    .iter()
                    .map(|(_, ty)| self.resolve_decl_type(Some(*ty)))
                    .collect(),
                ret: Box::new(ret_ty),
            };
            let symbol = self.bind_symbol(*name, fn_ty, true);
            self.predeclared_fns.insert(*root, symbol);
        }
    }

    fn lower_stmt(&mut self, id: ArenaId) -> HirStmt {
        let Some(node) = self.ast.get(id) else {
            return HirStmt::Invalid {
                span: Span::new_unchecked(self.ast.file_id, 0, 0),
            };
        };

        match node {
            AstNode::LetDecl {
                name,
                ty,
                value,
                is_const,
                span,
                ..
            } => {
                let value = self.lower_expr(*value);
                let symbol_ty = self.resolve_decl_type(*ty);
                let symbol = self.bind_symbol(*name, symbol_ty, *is_const);
                HirStmt::Let {
                    symbol,
                    value,
                    is_const: *is_const,
                    span: *span,
                }
            }
            AstNode::FnDecl {
                name,
                params,
                param_defaults,
                return_ty,
                body,
                span,
            } => {
                let symbol = self.predeclared_fns.get(&id).copied();
                self.lower_fn_decl(*name, params, param_defaults, *return_ty, *body, *span, symbol)
            }
            AstNode::StructDecl { name, fields, span } => {
                let struct_name = match self.ast.get(*name) {
                    Some(AstNode::Identifier { name, .. }) => name.clone(),
                    _ => "<invalid>".to_string(),
                };
                let symbol = self.bind_symbol(*name, Type::Unknown, true);
                if let Some(sym) = self.symbols.symbol_mut(symbol) {
                    sym.ty = Type::Struct(symbol);
                }
                let fields = fields
                    .iter()
                    .filter_map(|(field_name_id, field_ty_id)| {
                        let field_name = match self.ast.get(*field_name_id) {
                            Some(AstNode::Identifier { name, .. }) => name.clone(),
                            _ => return None,
                        };
                        Some((field_name, self.resolve_decl_type(Some(*field_ty_id))))
                    })
                    .collect::<Vec<_>>();
                HirStmt::StructDecl {
                    symbol,
                    name: struct_name,
                    fields,
                    span: *span,
                }
            }
            AstNode::TraitDecl {
                name,
                methods,
                span,
            } => {
                let symbol = self.bind_symbol(*name, Type::Unknown, true);
                let trait_name = match self.ast.get(*name) {
                    Some(AstNode::Identifier { name, .. }) => name.clone(),
                    _ => "<invalid>".to_string(),
                };
                if let Some(sym) = self.symbols.symbol_mut(symbol) {
                    sym.ty = Type::Trait(symbol);
                }
                let mut sigs = Vec::new();
                for method_id in methods {
                    if let Some(AstNode::FnDecl {
                        name,
                        params,
                        return_ty,
                        ..
                    }) = self.ast.get(*method_id)
                    {
                        let method_name = match self.ast.get(*name) {
                            Some(AstNode::Identifier { name, .. }) => name.clone(),
                            _ => "<invalid>".to_string(),
                        };
                        let mut ptys = Vec::new();
                        let mut is_instance = false;
                        for (idx, (_, pty)) in params.iter().enumerate() {
                            let ty = self.resolve_decl_type(Some(*pty));
                            if idx == 0 && matches!(ty, Type::SelfType) {
                                is_instance = true;
                            }
                            ptys.push(ty);
                        }
                        let ret = self.resolve_decl_type(Some(*return_ty));
                        sigs.push((method_name, ptys, ret, is_instance));
                    }
                }
                HirStmt::TraitDecl {
                    symbol,
                    name: trait_name,
                    methods: sigs,
                    span: *span,
                }
            }
            AstNode::ImplBlock {
                target_ty,
                trait_ty,
                methods,
                span,
            } => {
                let target = self.resolve_decl_type(Some(*target_ty));
                let trait_target = trait_ty.map(|id| self.resolve_decl_type(Some(id)));
                let methods = methods.iter().map(|id| self.lower_stmt(*id)).collect();
                HirStmt::ImplBlock {
                    target,
                    trait_target,
                    methods,
                    span: *span,
                }
            }
            AstNode::TupleDestructureDecl {
                names,
                ty,
                value,
                span,
            } => {
                let value = self.lower_expr(*value);
                let names = names
                    .iter()
                    .map(|name| self.bind_destructure_symbol(*name))
                    .collect::<Vec<_>>();
                let ty = ty.and_then(|ty_id| match self.ast.get(ty_id) {
                    Some(AstNode::TypeName { name, .. }) => map_type_name(name),
                    _ => None,
                });
                HirStmt::TupleDestructure {
                    names,
                    ty,
                    value,
                    span: *span,
                }
            }
            AstNode::AssignStmt { target, value, span } => {
                let value = self.lower_expr(*value);
                match self.ast.get(*target) {
                    Some(AstNode::Identifier { .. }) => {
                        let symbol = self.resolve_assignment_target(*target);
                        HirStmt::Assign {
                            symbol,
                            value,
                            span: *span,
                        }
                    }
                    Some(AstNode::ArrayAccessExpr { base, index, .. }) => {
                        let symbol = self.resolve_assignment_target(*base);
                        let index = self.lower_expr(*index);
                        HirStmt::ArrayAssign {
                            symbol,
                            index,
                            value,
                            span: *span,
                        }
                    }
                    _ => {
                        self.push_error("invalid assignment target", *span);
                        HirStmt::Invalid { span: *span }
                    }
                }
            }
            AstNode::CompoundAssignStmt {
                target,
                op,
                value,
                span,
            } => {
                let symbol = self.resolve_assignment_target(*target);
                let rhs = self.lower_expr(*value);
                let lhs = self.insert_hir_expr(HirExpr::Var(symbol), self.node_span(*target));
                let binary = self.insert_hir_expr(
                    HirExpr::Binary {
                        op: map_compound_op(*op),
                        lhs,
                        rhs,
                    },
                    *span,
                );
                HirStmt::Assign {
                    symbol,
                    value: binary,
                    span: *span,
                }
            }
            AstNode::ExprStmt { expr, span } => {
                let expr = self.lower_expr(*expr);
                HirStmt::Expr { expr, span: *span }
            }
            AstNode::BlockStmt { statements, span } => {
                let stmts = self.lower_block_stmts(statements);
                HirStmt::Block { stmts, span: *span }
            }
            AstNode::IfStmt {
                condition,
                then_branch,
                else_branch,
                span,
            } => {
                let condition = self.lower_expr(*condition);
                let then_branch = self.lower_if_branch(*then_branch);
                let else_branch = else_branch.map(|id| self.lower_if_branch(id));
                HirStmt::If {
                    condition,
                    then_branch,
                    else_branch,
                    span: *span,
                }
            }
            AstNode::WhileStmt {
                condition,
                body,
                span,
            } => {
                let condition = self.lower_expr(*condition);
                let body = self.lower_if_branch(*body);
                HirStmt::While {
                    condition,
                    body,
                    span: *span,
                }
            }
            AstNode::ForStmt {
                init,
                condition,
                step,
                body,
                span,
            } => {
                let init = init.map(|id| Box::new(self.lower_stmt(id)));
                let condition = condition.map(|id| self.lower_expr(id));
                let step = step.map(|id| self.lower_expr(id));
                let body = self.lower_if_branch(*body);
                HirStmt::For {
                    init,
                    condition,
                    step,
                    body,
                    span: *span,
                }
            }
            AstNode::ForInStmt {
                name,
                ty,
                iterable,
                body,
                span,
            } => {
                let symbol_ty = self.resolve_decl_type(*ty);
                let symbol = self.bind_symbol(*name, symbol_ty, false);
                let iterable_symbol = self.symbols.define(
                    self.current_scope,
                    "__forin_iterable".to_string(),
                    Type::Unknown,
                    SymbolOrigin::User,
                    false,
                );
                let index_symbol = self.symbols.define(
                    self.current_scope,
                    "__forin_index".to_string(),
                    Type::Int { signed: true, bits: 64 },
                    SymbolOrigin::User,
                    false,
                );
                let iterable = self.lower_expr(*iterable);
                let body = self.lower_if_branch(*body);
                HirStmt::ForIn {
                    symbol,
                    iterable_symbol,
                    index_symbol,
                    iterable,
                    body,
                    span: *span,
                }
            }
            AstNode::BreakStmt { span } => HirStmt::Break { span: *span },
            AstNode::ContinueStmt { span } => HirStmt::Continue { span: *span },
            AstNode::ReturnStmt { values, span } => {
                let values = values.iter().map(|id| self.lower_expr(*id)).collect();
                HirStmt::Return { values, span: *span }
            }
            AstNode::Invalid { span } => HirStmt::Invalid { span: *span },
            other => {
                let expr = self.lower_expr(id);
                HirStmt::Expr {
                    expr,
                    span: other.span(),
                }
            }
        }
    }

    fn bind_symbol(&mut self, id: ArenaId, ty: Type, is_const: bool) -> SymbolId {
        let Some(AstNode::Identifier { name, .. }) = self.ast.get(id) else {
            self.push_error("invalid declaration name", self.node_span(id));
            return self.symbols.define(
                self.current_scope,
                "<invalid>".to_string(),
                ty,
                SymbolOrigin::User,
                is_const,
            );
        };

        if self.symbols.resolve_in_scope(self.current_scope, name).is_some() {
            self.push_error(
                format!("symbol '{name}' already defined in scope"),
                self.node_span(id),
            );
        }

        self.symbols.define(
            self.current_scope,
            name.clone(),
            ty,
            SymbolOrigin::User,
            is_const,
        )
    }

    fn resolve_assignment_target(&mut self, id: ArenaId) -> SymbolId {
        let Some(AstNode::Identifier { name, span }) = self.ast.get(id) else {
            self.push_error("invalid assignment target", self.node_span(id));
            return self
                .symbols
                .define(self.current_scope, "<invalid_assign>".to_string(), Type::Unknown, SymbolOrigin::User, false);
        };
        match self.symbols.resolve(self.current_scope, name) {
            Some(symbol_id) => symbol_id,
            None => {
                self.push_error(format!("undefined symbol '{name}'"), *span);
                self.symbols
                    .define(self.current_scope, "<undefined_assign>".to_string(), Type::Unknown, SymbolOrigin::User, false)
            }
        }
    }

    fn bind_destructure_symbol(&mut self, id: ArenaId) -> SymbolId {
        if let Some(AstNode::Identifier { name, .. }) = self.ast.get(id) {
            if name == "_" {
                let synth = format!("__discard_{}", self.discard_counter);
                self.discard_counter = self.discard_counter.saturating_add(1);
                return self
                    .symbols
                    .define(self.current_scope, synth, Type::Unknown, SymbolOrigin::User, false);
            }
        }
        self.bind_symbol(id, Type::Unknown, false)
    }

    fn lower_expr(&mut self, id: ArenaId) -> ArenaId {
        let Some(node) = self.ast.get(id) else {
            return self.insert_hir_expr(
                HirExpr::Invalid,
                Span::new_unchecked(self.ast.file_id, 0, 0),
            );
        };

        match node {
            AstNode::IntegerLiteral { value, .. } => {
                self.insert_hir_expr(HirExpr::Int(value.clone()), self.node_span(id))
            }
            AstNode::FloatLiteral { value, .. } => {
                self.insert_hir_expr(HirExpr::Float(value.clone()), self.node_span(id))
            }
            AstNode::StringLiteral { value, .. } => {
                let value = unquote_string_literal(value);
                self.insert_hir_expr(HirExpr::Str(value), self.node_span(id))
            }
            AstNode::CharLiteral { value, .. } => {
                self.insert_hir_expr(HirExpr::Char(*value), self.node_span(id))
            }
            AstNode::BoolLiteral { value, .. } => {
                self.insert_hir_expr(HirExpr::Bool(*value), self.node_span(id))
            }
            AstNode::NullLiteral { .. } => self.insert_hir_expr(HirExpr::Null, self.node_span(id)),
            AstNode::UnaryExpr { op, operand, .. } => {
                let operand = self.lower_expr(*operand);
                let op = match op {
                    UnaryOp::Neg => HirUnaryOp::Neg,
                    UnaryOp::Not => HirUnaryOp::Not,
                    UnaryOp::BitNot => HirUnaryOp::BitNot,
                };
                self.insert_hir_expr(HirExpr::Unary { op, operand }, self.node_span(id))
            }
            AstNode::Identifier { name, span } => match self.symbols.resolve(self.current_scope, name) {
                Some(symbol_id) => self.insert_hir_expr(HirExpr::Var(symbol_id), self.node_span(id)),
                None => {
                    self.push_error(format!("undefined symbol '{name}'"), *span);
                    self.insert_hir_expr(HirExpr::Invalid, *span)
                }
            },
            AstNode::BinaryExpr {
                op, left, right, ..
            } => {
                let lhs = self.lower_expr(*left);
                let rhs = self.lower_expr(*right);
                self.insert_hir_expr(
                    HirExpr::Binary {
                        op: map_binary_op(*op),
                        lhs,
                        rhs,
                    },
                    self.node_span(id),
                )
            }
            AstNode::RangeExpr {
                start,
                end,
                inclusive,
                ..
            } => {
                let start = self.lower_expr(*start);
                let end = self.lower_expr(*end);
                self.insert_hir_expr(
                    HirExpr::Range {
                        start,
                        end,
                        inclusive: *inclusive,
                    },
                    self.node_span(id),
                )
            }
            AstNode::CallExpr { callee, args, span: _ } => {
                let lowered_args = args.iter().map(|arg| self.lower_expr(*arg)).collect::<Vec<_>>();
                let callee_expr = self.lower_expr(*callee);
                self.insert_hir_expr(
                    HirExpr::Call {
                        callee: callee_expr,
                        args: lowered_args,
                    },
                    self.node_span(id),
                )
            }
            AstNode::MethodCallExpr {
                receiver,
                method,
                args,
                ..
            } => {
                let receiver = self.lower_expr(*receiver);
                let args = args.iter().map(|arg| self.lower_expr(*arg)).collect();
                self.insert_hir_expr(
                    HirExpr::MethodCall {
                        receiver,
                        method: method.clone(),
                        args,
                    },
                    self.node_span(id),
                )
            }
            AstNode::StaticMethodCallExpr {
                type_name,
                method,
                args,
                ..
            } => {
                let args = args.iter().map(|arg| self.lower_expr(*arg)).collect();
                self.insert_hir_expr(
                    HirExpr::StaticMethodCall {
                        type_name: type_name.clone(),
                        method: method.clone(),
                        args,
                    },
                    self.node_span(id),
                )
            }
            AstNode::StructLiteralExpr {
                type_name, fields, ..
            } => {
                let fields = fields
                    .iter()
                    .map(|(name, expr)| (name.clone(), self.lower_expr(*expr)))
                    .collect();
                self.insert_hir_expr(
                    HirExpr::StructLiteral {
                        type_name: type_name.clone(),
                        fields,
                    },
                    self.node_span(id),
                )
            }
            AstNode::FieldAccessExpr { base, field, .. } => {
                let base = self.lower_expr(*base);
                self.insert_hir_expr(
                    HirExpr::FieldAccess {
                        base,
                        field: field.clone(),
                    },
                    self.node_span(id),
                )
            }
            AstNode::TupleLiteral { items, .. } => {
                let items = items.iter().map(|item| self.lower_expr(*item)).collect::<Vec<_>>();
                self.insert_hir_expr(HirExpr::Tuple(items), self.node_span(id))
            }
            AstNode::TupleAccess { tuple, index, .. } => {
                let tuple = self.lower_expr(*tuple);
                self.insert_hir_expr(
                    HirExpr::TupleAccess {
                        tuple,
                        index: *index,
                    },
                    self.node_span(id),
                )
            }
            AstNode::ArrayLiteral { items, .. } => {
                let items = items
                    .iter()
                    .map(|item| match item {
                        AstArrayItem::Expr(id) => HirArrayItem::Expr(self.lower_expr(*id)),
                        AstArrayItem::SpreadExpr(id) => HirArrayItem::SpreadExpr(self.lower_expr(*id)),
                    })
                    .collect::<Vec<_>>();
                self.insert_hir_expr(HirExpr::Array(items), self.node_span(id))
            }
            AstNode::ArrayAccessExpr { base, index, .. } => {
                let array = self.lower_expr(*base);
                let index = self.lower_expr(*index);
                self.insert_hir_expr(HirExpr::ArrayAccess { array, index }, self.node_span(id))
            }
            AstNode::IncDecExpr {
                target,
                op,
                position,
                span,
            } => {
                let symbol = self.resolve_assignment_target(*target);
                self.insert_hir_expr(
                    HirExpr::IncDec {
                        symbol,
                        op: map_inc_dec_op(*op),
                        position: map_inc_dec_position(*position),
                    },
                    *span,
                )
            }
            AstNode::PropagateExpr { expr, span } => {
                let expr = self.lower_expr(*expr);
                self.insert_hir_expr(HirExpr::Propagate { expr }, *span)
            }
            AstNode::TryCatchExpr {
                try_expr,
                err_name,
                err_ty,
                catch_block,
                span,
            } => {
                let try_expr = self.lower_expr(*try_expr);
                let err_ty = self.resolve_decl_type(Some(*err_ty));
                let parent_scope = self.current_scope;
                let catch_scope = self.symbols.create_scope(Some(parent_scope));
                self.current_scope = catch_scope;
                let err_symbol = self.bind_symbol(*err_name, err_ty, false);
                let (catch_stmts, catch_value) = self.lower_catch_block_with_value(*catch_block, *span);
                self.current_scope = parent_scope;
                self.insert_hir_expr(
                    HirExpr::TryCatch {
                        try_expr,
                        err_symbol,
                        catch_stmts,
                        catch_value,
                    },
                    *span,
                )
            }
            AstNode::TypeName { .. } => {
                self.push_error("type name is not an expression", self.node_span(id));
                self.insert_hir_expr(HirExpr::Invalid, self.node_span(id))
            }
            AstNode::SelfTypeRef { .. } => {
                self.push_error("Self type is not an expression", self.node_span(id));
                self.insert_hir_expr(HirExpr::Invalid, self.node_span(id))
            }
            AstNode::LetDecl { .. }
            | AstNode::TupleDestructureDecl { .. }
            | AstNode::FnDecl { .. }
            | AstNode::StructDecl { .. }
            | AstNode::TraitDecl { .. }
            | AstNode::ImplBlock { .. }
            | AstNode::AssignStmt { .. }
            | AstNode::CompoundAssignStmt { .. }
            | AstNode::IfStmt { .. }
            | AstNode::WhileStmt { .. }
            | AstNode::ForStmt { .. }
            | AstNode::ForInStmt { .. }
            | AstNode::BreakStmt { .. }
            | AstNode::ContinueStmt { .. }
            | AstNode::ReturnStmt { .. }
            | AstNode::ExprStmt { .. }
            | AstNode::BlockStmt { .. } => {
                self.push_error("statement used where expression expected", self.node_span(id));
                self.insert_hir_expr(HirExpr::Invalid, self.node_span(id))
            }
            AstNode::Invalid { .. } => self.insert_hir_expr(HirExpr::Invalid, self.node_span(id)),
        }
    }

    fn lower_fn_decl(
        &mut self,
        name_id: ArenaId,
        params: &[(ArenaId, ArenaId)],
        param_defaults: &[Option<ArenaId>],
        return_ty_id: ArenaId,
        body_id: ArenaId,
        span: Span,
        predeclared_symbol: Option<SymbolId>,
    ) -> HirStmt {
        let fn_name = match self.ast.get(name_id) {
            Some(AstNode::Identifier { name, .. }) => name.clone(),
            _ => "<invalid>".to_string(),
        };
        let is_instance = params
            .first()
            .map(|(_, ty)| matches!(self.ast.get(*ty), Some(AstNode::SelfTypeRef { .. })))
            .unwrap_or(false);
        let ret_ty = self.resolve_decl_type(Some(return_ty_id));
        let fn_ty = Type::Function {
            params: params
                .iter()
                .map(|(_, ty)| self.resolve_decl_type(Some(*ty)))
                .collect(),
            ret: Box::new(ret_ty.clone()),
        };
        let symbol = predeclared_symbol.unwrap_or_else(|| self.bind_symbol(name_id, fn_ty, true));

        let parent_scope = self.current_scope;
        let fn_scope = self.symbols.create_scope(Some(parent_scope));
        self.current_scope = fn_scope;

        let mut param_symbols = Vec::with_capacity(params.len());
        let mut lowered_defaults = Vec::with_capacity(params.len());
        for (param_name, param_ty_id) in params {
            let param_ty = self.resolve_decl_type(Some(*param_ty_id));
            let param_symbol = self.bind_symbol(*param_name, param_ty, false);
            param_symbols.push(param_symbol);
        }
        for default in param_defaults {
            lowered_defaults.push(default.map(|d| self.lower_expr(d)));
        }

        let body = self.lower_if_branch(body_id);
        self.current_scope = parent_scope;
        HirStmt::FnDecl {
            symbol,
            name: fn_name,
            is_instance,
            params: param_symbols,
            param_defaults: lowered_defaults,
            return_ty: ret_ty,
            body,
            span,
        }
    }

    fn insert_hir_expr(&mut self, expr: HirExpr, span: Span) -> ArenaId {
        let id = self
            .hir
            .exprs
            .insert(expr)
            .expect("hir arena insertion should not fail in normal conditions");
        self.hir.expr_spans.insert(id, span);
        id
    }

    fn node_span(&self, id: ArenaId) -> Span {
        self.ast
            .get(id)
            .map(AstNode::span)
            .unwrap_or_else(|| Span::new_unchecked(self.ast.file_id, 0, 0))
    }

    fn push_error(&mut self, message: impl Into<String>, span: Span) {
        self.diagnostics
            .push(Diagnostic::new(message, span, Severity::Error));
    }

    fn resolve_decl_type(&mut self, ty: Option<ArenaId>) -> Type {
        let Some(ty_id) = ty else {
            return Type::Unknown;
        };
        match self.ast.get(ty_id) {
            Some(AstNode::SelfTypeRef { .. }) => Type::SelfType,
            Some(AstNode::TypeName { name, span }) => match map_type_name(name) {
                Some(ty) => ty,
                None => {
                    if let Some(symbol) = self.symbols.resolve(self.current_scope, name) {
                        self.symbols
                            .symbol(symbol)
                            .map(|s| s.ty.clone())
                            .unwrap_or(Type::Unknown)
                    } else {
                        self.push_error(format!("unknown type '{name}'"), *span);
                        Type::Unknown
                    }
                }
            },
            _ => Type::Unknown,
        }
    }

    fn lower_block_stmts(&mut self, statements: &[ArenaId]) -> Vec<HirStmt> {
        let parent_scope = self.current_scope;
        let block_scope = self.symbols.create_scope(Some(parent_scope));
        self.current_scope = block_scope;
        let lowered = statements
            .iter()
            .map(|stmt_id| self.lower_stmt(*stmt_id))
            .collect::<Vec<_>>();
        self.current_scope = parent_scope;
        lowered
    }

    fn lower_if_branch(&mut self, branch_id: ArenaId) -> Vec<HirStmt> {
        match self.ast.get(branch_id) {
            Some(AstNode::BlockStmt { statements, .. }) => self.lower_block_stmts(statements),
            Some(AstNode::IfStmt { .. }) => vec![self.lower_stmt(branch_id)],
            Some(AstNode::Invalid { span }) => {
                self.push_error("invalid if branch", *span);
                vec![HirStmt::Invalid { span: *span }]
            }
            _ => {
                self.push_error("invalid if branch", self.node_span(branch_id));
                vec![HirStmt::Invalid {
                    span: self.node_span(branch_id),
                }]
            }
        }
    }

    fn lower_catch_block_with_value(&mut self, block_id: ArenaId, span: Span) -> (Vec<HirStmt>, ArenaId) {
        let Some(AstNode::BlockStmt { statements, .. }) = self.ast.get(block_id) else {
            self.push_error("catch requires a block", self.node_span(block_id));
            return (Vec::new(), self.insert_hir_expr(HirExpr::Invalid, span));
        };
        let mut lowered = Vec::new();
        let mut value = None;
        for (idx, stmt_id) in statements.iter().enumerate() {
            let is_last = idx + 1 == statements.len();
            match self.ast.get(*stmt_id) {
                Some(AstNode::ReturnStmt { values, span: ret_span }) if is_last => {
                    if values.len() != 1 {
                        self.push_error("catch return must contain exactly one value", *ret_span);
                        value = Some(self.insert_hir_expr(HirExpr::Invalid, *ret_span));
                    } else {
                        value = Some(self.lower_expr(values[0]));
                    }
                }
                Some(AstNode::ReturnStmt { span: ret_span, .. }) => {
                    self.push_error("catch return must be the last statement in catch block", *ret_span);
                    lowered.push(HirStmt::Invalid { span: *ret_span });
                }
                _ => lowered.push(self.lower_stmt(*stmt_id)),
            }
        }
        let value = value.unwrap_or_else(|| {
            self.push_error("catch block must end with `return <value>`", span);
            self.insert_hir_expr(HirExpr::Invalid, span)
        });
        (lowered, value)
    }
}

fn map_inc_dec_op(op: AstIncDecOp) -> HirIncDecOp {
    match op {
        AstIncDecOp::Increment => HirIncDecOp::Increment,
        AstIncDecOp::Decrement => HirIncDecOp::Decrement,
    }
}

fn map_inc_dec_position(position: AstIncDecPosition) -> HirIncDecPosition {
    match position {
        AstIncDecPosition::Prefix => HirIncDecPosition::Prefix,
        AstIncDecPosition::Postfix => HirIncDecPosition::Postfix,
    }
}

fn map_binary_op(op: BinaryOp) -> BinOp {
    match op {
        BinaryOp::Add => BinOp::Add,
        BinaryOp::Subtract => BinOp::Subtract,
        BinaryOp::Multiply => BinOp::Multiply,
        BinaryOp::Divide => BinOp::Divide,
        BinaryOp::Modulo => BinOp::Modulo,
        BinaryOp::Power => BinOp::Power,
        BinaryOp::Equal => BinOp::Equal,
        BinaryOp::NotEqual => BinOp::NotEqual,
        BinaryOp::Less => BinOp::Less,
        BinaryOp::LessEqual => BinOp::LessEqual,
        BinaryOp::Greater => BinOp::Greater,
        BinaryOp::GreaterEqual => BinOp::GreaterEqual,
        BinaryOp::LogicalAnd => BinOp::LogicalAnd,
        BinaryOp::LogicalOr => BinOp::LogicalOr,
        BinaryOp::BitAnd => BinOp::BitAnd,
        BinaryOp::BitOr => BinOp::BitOr,
        BinaryOp::BitXor => BinOp::BitXor,
        BinaryOp::ShiftLeft => BinOp::ShiftLeft,
        BinaryOp::ShiftRight => BinOp::ShiftRight,
    }
}

fn map_compound_op(op: CompoundOp) -> BinOp {
    match op {
        CompoundOp::Add => BinOp::Add,
        CompoundOp::Subtract => BinOp::Subtract,
        CompoundOp::Multiply => BinOp::Multiply,
        CompoundOp::Divide => BinOp::Divide,
        CompoundOp::Modulo => BinOp::Modulo,
        CompoundOp::Power => BinOp::Power,
        CompoundOp::BitAnd => BinOp::BitAnd,
        CompoundOp::BitOr => BinOp::BitOr,
        CompoundOp::BitXor => BinOp::BitXor,
        CompoundOp::ShiftLeft => BinOp::ShiftLeft,
        CompoundOp::ShiftRight => BinOp::ShiftRight,
    }
}

fn init_global_scope(
    symbols: &mut SymbolTable,
    scope_id: ScopeId,
    registry: &crate::builtins::BuiltinRegistry,
) {
    for builtin in &registry.functions {
        symbols.define(
            scope_id,
            builtin.name.to_string(),
            builtin.ty.clone(),
            SymbolOrigin::Builtin,
            true,
        );
    }
}

/// Lexer includes surrounding `"`; runtime `print` expects the decoded content (no delimiter).
fn unquote_string_literal(raw: &str) -> String {
    let Some(inner) = raw.strip_prefix('"').and_then(|s| s.strip_suffix('"')) else {
        return raw.to_string();
    };
    let mut out = String::with_capacity(inner.len());
    let mut chars = inner.chars();
    while let Some(ch) = chars.next() {
        if ch != '\\' {
            out.push(ch);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('r') => out.push('\r'),
            Some('"') => out.push('"'),
            Some('\\') => out.push('\\'),
            Some(other) => {
                out.push('\\');
                out.push(other);
            }
            None => out.push('\\'),
        }
    }
    out
}

fn map_type_name(name: &str) -> Option<Type> {
    if let Some(array_ty) = parse_array_type_name(name) {
        return Some(array_ty);
    }
    if let Some(tuple_ty) = parse_tuple_type_name(name) {
        return Some(tuple_ty);
    }
    if let Some(fn_ty) = parse_function_type_name(name) {
        return Some(fn_ty);
    }
    match name {
        "i1" => Some(Type::Int { signed: true, bits: 1 }),
        "i8" => Some(Type::Int { signed: true, bits: 8 }),
        "i16" => Some(Type::Int { signed: true, bits: 16 }),
        "i32" => Some(Type::Int { signed: true, bits: 32 }),
        "i64" => Some(Type::Int { signed: true, bits: 64 }),
        "i128" => Some(Type::Int { signed: true, bits: 128 }),
        "u1" => Some(Type::Int { signed: false, bits: 1 }),
        "u8" => Some(Type::Int { signed: false, bits: 8 }),
        "u16" => Some(Type::Int { signed: false, bits: 16 }),
        "u32" => Some(Type::Int { signed: false, bits: 32 }),
        "u64" => Some(Type::Int { signed: false, bits: 64 }),
        "u128" => Some(Type::Int { signed: false, bits: 128 }),
        "f32" => Some(Type::Float { bits: 32 }),
        "f64" => Some(Type::Float { bits: 64 }),
        "str" => Some(Type::Str),
        "bool" => Some(Type::Bool),
        "char" => Some(Type::Char),
        "unit" | "void" => Some(Type::Unit),
        "null" => Some(Type::Null),
        "err" => Some(Type::Err),
        "Self" => Some(Type::SelfType),
        _ => None,
    }
}

fn parse_array_type_name(name: &str) -> Option<Type> {
    if !(name.starts_with('[') && name.ends_with(']')) {
        return None;
    }
    let inner = name[1..name.len() - 1].trim();
    if inner.is_empty() {
        return None;
    }
    let item = map_type_name(inner)?;
    Some(Type::Array(Box::new(item)))
}

fn parse_tuple_type_name(name: &str) -> Option<Type> {
    if !(name.starts_with('(') && name.ends_with(')')) {
        return None;
    }
    let inner = &name[1..name.len() - 1];
    if inner.trim().is_empty() {
        return None;
    }
    let mut depth = 0usize;
    let mut parts = Vec::new();
    let mut start = 0usize;
    for (idx, ch) in inner.char_indices() {
        match ch {
            '(' => depth += 1,
            ')' => depth = depth.saturating_sub(1),
            ',' if depth == 0 => {
                parts.push(inner[start..idx].trim().to_string());
                start = idx + 1;
            }
            _ => {}
        }
    }
    parts.push(inner[start..].trim().to_string());
    if parts.len() < 2 {
        return None;
    }
    let mut tys = Vec::with_capacity(parts.len());
    for part in parts {
        tys.push(map_type_name(&part)?);
    }
    Some(Type::Tuple(tys))
}

fn parse_function_type_name(name: &str) -> Option<Type> {
    let fn_prefix = "fn(";
    if !name.starts_with(fn_prefix) {
        return None;
    }
    let close_paren = name.find(')')?;
    let params_src = &name[fn_prefix.len()..close_paren];
    let arrow_src = name.get(close_paren + 1..)?.trim_start();
    let return_src = arrow_src.strip_prefix("->")?.trim();

    let mut params = Vec::new();
    if !params_src.trim().is_empty() {
        for piece in params_src.split(',') {
            let ty = map_type_name(piece.trim())?;
            params.push(ty);
        }
    }
    let ret = map_type_name(return_src)?;
    Some(Type::Function {
        params,
        ret: Box::new(ret),
    })
}

#[cfg(test)]
mod tests {
    use foundation::ids::FileId;

    use crate::{
        hir::{BinOp, HirExpr, HirStmt, ScopeId, SymbolOrigin},
        lexer::lex,
        parser::parse,
    };

    use super::lower;

    #[test]
    fn lowers_identifiers_to_symbols() {
        let src = "a := 1; b := a + 2";
        let lex_output = lex(FileId::from_u32(1), src);
        let (ast, parser_diagnostics) = parse(FileId::from_u32(1), src.len() as u32, lex_output.tokens);
        assert!(!parser_diagnostics.has_errors());

        let (hir, symbols, diagnostics) = lower(&ast);
        assert!(!diagnostics.has_errors());
        assert_eq!(hir.stmts.len(), 2);
        let b_symbol = symbols.resolve(ScopeId(0), "b");
        assert!(b_symbol.is_some());
    }

    #[test]
    fn reports_undefined_symbol() {
        let src = "a := b + 1";
        let lex_output = lex(FileId::from_u32(2), src);
        let (ast, _) = parse(FileId::from_u32(2), src.len() as u32, lex_output.tokens);

        let (hir, _symbols, diagnostics) = lower(&ast);
        assert!(diagnostics.has_errors());
        let mut has_invalid = false;
        for idx in 0..hir.exprs.len() {
            let id = foundation::ids::ArenaId::from_u32(idx as u32);
            if matches!(hir.exprs.get(id), Some(HirExpr::Invalid)) {
                has_invalid = true;
                break;
            }
        }
        assert!(has_invalid);
        assert!(diagnostics.iter().any(|d| d.message.contains("undefined symbol")));
    }

    #[test]
    fn hir_var_uses_symbol_id_not_string() {
        let src = "a := 1; b := a";
        let lex_output = lex(FileId::from_u32(3), src);
        let (ast, _) = parse(FileId::from_u32(3), src.len() as u32, lex_output.tokens);
        let (hir, _symbols, diagnostics) = lower(&ast);
        assert!(!diagnostics.has_errors());

        let mut has_var = false;
        for idx in 0..hir.exprs.len() {
            let id = foundation::ids::ArenaId::from_u32(idx as u32);
            if matches!(hir.exprs.get(id), Some(HirExpr::Var(_))) {
                has_var = true;
                break;
            }
        }
        assert!(has_var);
    }

    #[test]
    fn global_scope_includes_print_builtin() {
        let src = "a := 1";
        let lex_output = lex(FileId::from_u32(4), src);
        let (ast, _) = parse(FileId::from_u32(4), src.len() as u32, lex_output.tokens);
        let (_hir, symbols, diagnostics) = lower(&ast);
        assert!(!diagnostics.has_errors());
        let print_id = symbols.resolve(ScopeId(0), "print").expect("builtin print");
        let print_symbol = symbols.symbol(print_id).expect("symbol exists");
        assert_eq!(print_symbol.origin, SymbolOrigin::Builtin);
    }

    #[test]
    fn lowers_builtin_call_into_hir_call() {
        let src = "print(1, 2)";
        let lex_output = lex(FileId::from_u32(5), src);
        let (ast, _) = parse(FileId::from_u32(5), src.len() as u32, lex_output.tokens);
        let (hir, _symbols, diagnostics) = lower(&ast);
        assert!(!diagnostics.has_errors());

        let mut has_call = false;
        for idx in 0..hir.exprs.len() {
            let id = foundation::ids::ArenaId::from_u32(idx as u32);
            if matches!(hir.exprs.get(id), Some(HirExpr::Call { .. })) {
                has_call = true;
                break;
            }
        }
        assert!(has_call);
    }

    #[test]
    fn block_scope_allows_shadowing_without_leaking() {
        let src = "x := 1; { x := 2; y := x }; z := x";
        let lex_output = lex(FileId::from_u32(6), src);
        let (ast, _) = parse(FileId::from_u32(6), src.len() as u32, lex_output.tokens);
        let (_hir, _symbols, diagnostics) = lower(&ast);
        assert!(!diagnostics.has_errors());
    }

    #[test]
    fn block_local_is_not_visible_outside() {
        let src = "{ local := 1 }; print(local)";
        let lex_output = lex(FileId::from_u32(7), src);
        let (ast, _) = parse(FileId::from_u32(7), src.len() as u32, lex_output.tokens);
        let (_hir, _symbols, diagnostics) = lower(&ast);
        assert!(diagnostics.has_errors());
        assert!(diagnostics.iter().any(|d| d.message.contains("undefined symbol 'local'")));
    }

    #[test]
    fn lowers_new_binary_ops_into_hir() {
        let src = "x := (1 % 2) == 1 && true || false";
        let lex_output = lex(FileId::from_u32(8), src);
        let (ast, _) = parse(FileId::from_u32(8), src.len() as u32, lex_output.tokens);
        let (hir, _symbols, diagnostics) = lower(&ast);
        assert!(!diagnostics.has_errors());
        let mut has_mod = false;
        let mut has_eq = false;
        let mut has_and = false;
        let mut has_or = false;
        for idx in 0..hir.exprs.len() {
            let id = foundation::ids::ArenaId::from_u32(idx as u32);
            if let Some(HirExpr::Binary { op, .. }) = hir.exprs.get(id) {
                has_mod |= matches!(op, BinOp::Modulo);
                has_eq |= matches!(op, BinOp::Equal);
                has_and |= matches!(op, BinOp::LogicalAnd);
                has_or |= matches!(op, BinOp::LogicalOr);
            }
        }
        assert!(has_mod && has_eq && has_and && has_or);
    }

    #[test]
    fn lowers_new_unary_ops_into_hir() {
        let src = "x := !false; y := ~1";
        let lex_output = lex(FileId::from_u32(9), src);
        let (ast, _) = parse(FileId::from_u32(9), src.len() as u32, lex_output.tokens);
        let (hir, _symbols, diagnostics) = lower(&ast);
        assert!(!diagnostics.has_errors());
        let mut has_not = false;
        let mut has_bit_not = false;
        for idx in 0..hir.exprs.len() {
            let id = foundation::ids::ArenaId::from_u32(idx as u32);
            if let Some(HirExpr::Unary { op, .. }) = hir.exprs.get(id) {
                has_not |= matches!(op, crate::hir::UnaryOp::Not);
                has_bit_not |= matches!(op, crate::hir::UnaryOp::BitNot);
            }
        }
        assert!(has_not && has_bit_not);
    }

    #[test]
    fn lowers_if_else_stmt_into_hir_if() {
        let src = "if true { x := 1 } else { x := 2 }";
        let lex_output = lex(FileId::from_u32(10), src);
        let (ast, _) = parse(FileId::from_u32(10), src.len() as u32, lex_output.tokens);
        let (hir, _symbols, diagnostics) = lower(&ast);
        assert!(!diagnostics.has_errors());
        assert!(matches!(hir.stmts.first(), Some(HirStmt::If { .. })));
    }

    #[test]
    fn lowers_while_break_continue_into_hir() {
        let src = "while 1 { continue; break }";
        let lex_output = lex(FileId::from_u32(11), src);
        let (ast, _) = parse(FileId::from_u32(11), src.len() as u32, lex_output.tokens);
        let (hir, _symbols, diagnostics) = lower(&ast);
        assert!(!diagnostics.has_errors());
        let Some(HirStmt::While { body, .. }) = hir.stmts.first() else {
            panic!("expected while");
        };
        assert!(body.iter().any(|s| matches!(s, HirStmt::Continue { .. })));
        assert!(body.iter().any(|s| matches!(s, HirStmt::Break { .. })));
    }

    #[test]
    fn lowers_compound_assign_to_binary_assign() {
        let src = "x := 1; x += 2";
        let lex_output = lex(FileId::from_u32(12), src);
        let (ast, _) = parse(FileId::from_u32(12), src.len() as u32, lex_output.tokens);
        let (hir, _symbols, diagnostics) = lower(&ast);
        assert!(!diagnostics.has_errors());
        let Some(HirStmt::Assign { value, .. }) = hir.stmts.get(1) else {
            panic!("expected assign stmt");
        };
        assert!(matches!(
            hir.exprs.get(*value),
            Some(HirExpr::Binary { op: BinOp::Add, .. })
        ));
    }

    #[test]
    fn lowers_for_stmt_into_hir_for() {
        let src = "for i: i32 = 0; i < 2; i++ { print(i) }";
        let lex_output = lex(FileId::from_u32(13), src);
        let (ast, _) = parse(FileId::from_u32(13), src.len() as u32, lex_output.tokens);
        let (hir, _symbols, diagnostics) = lower(&ast);
        assert!(!diagnostics.has_errors());
        assert!(matches!(hir.stmts.first(), Some(HirStmt::For { .. })));
    }

    #[test]
    fn lowers_incdec_expr_into_hir_incdec() {
        let src = "x: i32 = 1; y := x++";
        let lex_output = lex(FileId::from_u32(14), src);
        let (ast, _) = parse(FileId::from_u32(14), src.len() as u32, lex_output.tokens);
        let (hir, _symbols, diagnostics) = lower(&ast);
        assert!(!diagnostics.has_errors());
        let Some(HirStmt::Let { value, .. }) = hir.stmts.get(1) else {
            panic!("expected second let");
        };
        assert!(matches!(hir.exprs.get(*value), Some(HirExpr::IncDec { .. })));
    }

    #[test]
    fn lowers_function_decl_and_return() {
        let src = "fn add(a: i32, b: i32) -> i32 { return a + b }";
        let lex_output = lex(FileId::from_u32(15), src);
        let (ast, _) = parse(FileId::from_u32(15), src.len() as u32, lex_output.tokens);
        let (hir, _symbols, diagnostics) = lower(&ast);
        assert!(!diagnostics.has_errors());
        assert!(matches!(hir.stmts.first(), Some(HirStmt::FnDecl { .. })));
    }

    #[test]
    fn lowers_propagate_and_try_catch_exprs() {
        let src = r#"
            fn div(a: i32, b: i32) -> (i32, err) { return a / b, null }
            fn f() -> (i32, err) { x := div(4, 2)?; return x, null }
            y := try div(1, 0) catch(e: err) { print(e.message); return 0 }
        "#;
        let lex_output = lex(FileId::from_u32(60), src);
        let (ast, _) = parse(FileId::from_u32(60), src.len() as u32, lex_output.tokens);
        let (hir, _symbols, diagnostics) = lower(&ast);
        assert!(!diagnostics.has_errors());
        let mut has_propagate = false;
        let mut has_try_catch = false;
        for idx in 0..hir.exprs.len() {
            let id = foundation::ids::ArenaId::from_u32(idx as u32);
            has_propagate |= matches!(hir.exprs.get(id), Some(HirExpr::Propagate { .. }));
            has_try_catch |= matches!(hir.exprs.get(id), Some(HirExpr::TryCatch { .. }));
        }
        assert!(has_propagate && has_try_catch);
    }
}
